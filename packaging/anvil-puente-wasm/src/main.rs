//! Puente gRPC↔función (ADR-0015): sirve un componente `.wasm` de paso de
//! Anvil (interfaz `anvil:paso`, una función `run`) como un ejecutor gRPC
//! en loopback. El motor habla con él por el mismo `paso.proto` de siempre;
//! el `.wasm` del usuario no sabe de gRPC ni de protobuf — sólo exporta
//! `run(nombre, intento) -> resultado`.
//!
//! Uso:
//!   anvil-puente-wasm --wasm <ruta.wasm> [--port <puerto>] [--bind <ip>]
//!
//! - `--wasm`: path al componente (obligatorio).
//! - `--port`: puerto a escuchar. `anvil-host` siempre lo pasa concreto
//!   (reserva `127.0.0.1:0` antes de spawnear, como ya hacía con los
//!   `.wasm` gRPC). Por defecto 0 (efímero; útil sólo a mano, no hay
//!   forma de conocer el puerto asignado).
//! - `--bind`: IP a la que bindear; por defecto 127.0.0.1 (loopback-only).
//!   `--bind 0.0.0.0` habilita el caso remoto (Raspberry Pi).
//!
//! El puente es código NATIVO de Anvil: por eso puede usar tonic (a
//! diferencia de los guests WASM, que usan wasi-grpc). El usuario nunca lo
//! compila; anvil-host lo spawnea (embebido en el binario `anvil`) o se
//! distribuye suelto.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use tonic::{transport::Server, Request, Response, Status};
use wasmtime::component::{bindgen, Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

/// `paso.proto` compilado por `build.rs` con tonic (sin `package` en el
/// `.proto`, así que tonic genera el módulo raíz `_`).
pub mod pb {
    tonic::include_proto!("_");
}

use exports::anvil::step::step::Named;
use pb::step_executor_server::{StepExecutor, StepExecutorServer};
use pb::{Catalog, CatalogRequest, StepRequest, StepResult};

/// La versión de contrato que habla este puente (ADR-0020 §4).
///
/// **Está repetida a mano, y eso es exactamente lo que el ADR avisa que es
/// peligroso.** No se puede importar de `modelo` porque este crate no lo
/// linka a propósito: `modelo` usa prost 0.14 y el `pb` de aquí lo genera
/// tonic 0.12 con prost 0.13 (ver `Cargo.toml`).
///
/// Lo que impide que se desincronice es el test
/// `el_contrato_del_puente_es_el_de_modelo`, que sí linka `modelo` (como
/// dev-dependency, sin tocar el binario) y compara los dos números. Si
/// alguien sube el contrato en `modelo` y se olvida de aquí, ese test se pone
/// rojo — que es la diferencia entre un fallo de compilación y **un eco que
/// miente**.
const CONTRACT: i32 = 3;

// Bindings del WIT `anvil:paso` (el `wit/` de este crate es la fuente de
// verdad del contrato; el autor del componente usa el mismo fichero).
bindgen!({
    path: "wit",
    world: "anvil-step",
});

/// Estado del componente: contexto WASI (vacío: el componente es una
/// función pura, sin ficheros ni red) + tabla de recursos de wasmtime.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// El componente instanciado y listo para llamar `run`. Se carga **una vez**
/// al arrancar (preload, como TestStand) y se reutiliza para todas las
/// llamadas: 1 Store, N llamadas.
struct ComponenteCargado {
    store: Store<State>,
    paso: AnvilStep,
}

/// Distingue, mirando los 8 bytes de cabecera, los dos fallos que wasmtime
/// reporta con el **mismo** texto («failed to parse WebAssembly module»): un
/// fichero que no es WASM, y un módulo *core* compilado como tal en vez de
/// como componente. El segundo es el tropiezo nº1 de quien escribe su primer
/// paso, y el mensaje de wasmtime le hace culpar al toolchain (DIAG-5).
///
/// La cabecera es `\0asm` + versión: los módulos core llevan `01 00 00 00` y
/// los componentes `0d 00 01 00` (layer 1). Mirar los bytes, y no el texto del
/// error, deja el diagnóstico a salvo de que wasmtime lo reescriba.
fn diagnostica_no_componente(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
        return Some(
            "no es un fichero WebAssembly: no empieza por la cabecera '\\0asm'".to_string(),
        );
    }
    if bytes[4..8] == [0x01, 0x00, 0x00, 0x00] {
        return Some(
            "es un módulo core de WebAssembly, no un componente: compílalo con \
             'cargo component build' y comprueba que el crate llama a \
             'bindings::export!' (ver docs/guia-inicio-rapido.md)"
                .to_string(),
        );
    }
    None
}

impl ComponenteCargado {
    fn cargar(engine: &Engine, bytes: &[u8]) -> Result<Self, String> {
        // Antes de instanciar nada: si el `.wasm` no es un componente, decirlo
        // con el motivo real en vez del genérico de wasmtime.
        if let Some(diag) = diagnostica_no_componente(bytes) {
            return Err(diag);
        }
        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| format!("linker WASI: {e}"))?;
        // Sin preopens ni red: sandbox real. El componente sólo hace su
        // función; no toca el host.
        let state = State {
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
        };
        let mut store = Store::new(engine, state);
        let component = Component::from_binary(engine, bytes)
            .map_err(|e| format!("componente inválido: {e}"))?;
        let paso = AnvilStep::instantiate(&mut store, &component, &linker)
            .map_err(|e| format!("no se pudo instanciar: {e}"))?;
        Ok(ComponenteCargado { store, paso })
    }

    /// Llama a `run` del componente y traduce el resultado al protobuf del
    /// contrato. Un error del guest (pánico, trap) se reporta como
    /// `Status::internal`: el motor lo ve como error del paso, no corta la
    /// secuencia por red.
    // `clippy::result_large_err`: el `Err` es `tonic::Status` (176 bytes), el
    // tipo de error canónico de tonic. Boxearlo sólo para este método obligaría
    // a desenvolverlo en cada punto donde tonic lo espera, a cambio de nada:
    // esto se llama una vez por paso, no en un bucle caliente.
    #[allow(clippy::result_large_err)]
    fn llamar(
        &mut self,
        nombre: &str,
        intento: i32,
        parametros: &[Named],
    ) -> Result<StepResult, Status> {
        let r = self
            .paso
            .anvil_step_step()
            .call_run(&mut self.store, nombre, intento, parametros)
            .map_err(|e| Status::internal(format!("el paso '{nombre}' falló: {e}")))?;
        Ok(StepResult {
            name: nombre.to_string(),
            status: r.status,
            message: r.message,
            measured_value: r.measured_value.map(|v| v.to_string()).unwrap_or_default(),
            limit_min: String::new(),
            limit_max: String::new(),
            outputs: r.outputs.iter().map(nombrado_a_proto).collect(),
            // **El puente responde el eco por el componente** (ADR-0020 §4d).
            // Un componente no sabe de gRPC, de protobuf ni de versiones de
            // contrato (ADR-0015); el puente es el único que traduce, y por
            // tanto el único que sabe qué número de contrato corresponde a
            // qué versión del WIT. Si llegó hasta aquí es que el `.wasm`
            // casaba con `anvil:step@0.3.0` —wasmtime falla al instanciar si
            // no— así que habla el contrato de este binario.
            contract: CONTRACT,
        })
    }
}

/// El servicio gRPC `EjecutorPasos` (el contrato del motor). Cada `Invoca`
/// delega en el componente cargado.
///
/// El `Mutex` es estándar (bloqueante): la llamada a `run` de wasmtime es
/// síncrona y el motor es secuencial (una petición a la vez por cliente),
/// así que no hay contención. Si en el futuro entra paralelismo, conviene
/// `spawn_blocking` para no frenar el runtime async.
struct ServicioEjecutor {
    componente: Arc<Mutex<ComponenteCargado>>,
}

#[tonic::async_trait]
impl StepExecutor for ServicioEjecutor {
    async fn invoke(&self, request: Request<StepRequest>) -> Result<Response<StepResult>, Status> {
        let pet = request.into_inner();
        // Un parámetro cuyo `oneof` llegó sin rama no dice de qué tipo es. No
        // se puede pasar al componente ni omitirlo en silencio: omitirlo sería
        // que el paso midiera sin él y nadie se enterase (ADR-0019, Regla 2).
        let parametros = pet
            .inputs
            .iter()
            .map(proto_a_nombrado)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|n: String| {
                Status::invalid_argument(format!(
                    "el parámetro '{n}' llegó sin tipo (ninguna de las ramas numero/texto/booleano)"
                ))
            })?;
        let mut comp = self
            .componente
            .lock()
            .map_err(|_| Status::internal("componente en uso"))?;
        let respuesta = comp.llamar(&pet.name, pet.attempt, &parametros)?;
        Ok(Response::new(respuesta))
    }

    /// The bridge **cannot** describe the component it serves, and says so
    /// (ADR-0021 §4).
    ///
    /// `anvil:step@0.3.0` exports a single `run(name, attempt, inputs)`: the
    /// component dispatches by name inside itself, so from out here there is no
    /// list of names to publish and no signature to read. The WIT embedded in
    /// the `.wasm` does not help either — it would say *"there is a `run` that
    /// takes a name and a list"*, which is true and useless. It is the bill for
    /// dispatching by name (ADR-0003), the decision that makes Anvil
    /// language-agnostic.
    ///
    /// So it answers `describes = false`: those steps come out as unchecked in
    /// `--validate`, which is the honest answer. Making them describable means
    /// adding an introspection function to the WIT — a new version of the
    /// interface and a recompile of every component (ADR-0020 §4d), and that is
    /// the decision issue #39 is waiting on, not this one.
    async fn describe(&self, _r: Request<CatalogRequest>) -> Result<Response<Catalog>, Status> {
        Ok(Response::new(Catalog::default()))
    }
}

fn parse_args() -> Result<(PathBuf, u16, IpAddr), String> {
    let mut wasm: Option<PathBuf> = None;
    let mut port: u16 = 0;
    let mut bind: IpAddr = "127.0.0.1".parse().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wasm" => {
                i += 1;
                wasm = Some(PathBuf::from(args.get(i).ok_or("--wasm sin valor")?));
            }
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .ok_or("--port sin valor")?
                    .parse()
                    .map_err(|_| "--port no es un número")?;
            }
            "--bind" => {
                i += 1;
                bind = args
                    .get(i)
                    .ok_or("--bind sin valor")?
                    .parse()
                    .map_err(|_| "--bind no es una IP válida")?;
            }
            other => return Err(format!("flag desconocido: '{other}'")),
        }
        i += 1;
    }
    Ok((wasm.ok_or("falta --wasm <ruta.wasm>")?, port, bind))
}

fn main() {
    let (ruta_wasm, port, bind) = match parse_args() {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "uso: anvil-puente-wasm --wasm <ruta.wasm> [--port <puerto>] [--bind <ip>]\n{e}"
            );
            std::process::exit(2);
        }
    };

    let bytes = match std::fs::read(&ruta_wasm) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("no se pudo leer '{}': {e}", ruta_wasm.display());
            std::process::exit(1);
        }
    };

    let engine = Engine::default();

    // Preload: instancia el componente al arrancar (1 Store, N llamadas).
    let componente = match ComponenteCargado::cargar(&engine, &bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "no se pudo cargar el componente '{}': {e}",
                ruta_wasm.display()
            );
            std::process::exit(1);
        }
    };
    let componente = Arc::new(Mutex::new(componente));

    let addr = SocketAddr::new(bind, port);
    let servicio = ServicioEjecutor { componente };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("no se pudo crear el runtime: {e}");
            std::process::exit(1);
        }
    };

    let servidor = Server::builder()
        .add_service(StepExecutorServer::new(servicio))
        .serve(addr);

    eprintln!("anvil-puente-wasm: cargado '{}'", ruta_wasm.display());
    eprintln!("anvil-puente-wasm: escuchando en {addr}");

    // Salida limpia: el host spawnea el puente con stdin en pipe; cuando el
    // host muere (o droppea el `Child`), el pipe se cierra → EOF → el puente
    // sale. Sin esto, un host muerto dejaría huérfano al puente.
    let stdin_actual = std::io::stdin();
    let _ = thread::spawn(move || {
        let mut buf = [0u8; 1];
        while let Ok(n) = std::io::Read::read(&mut stdin_actual.lock(), &mut buf) {
            if n == 0 {
                std::process::exit(0);
            }
        }
    });

    // El servidor no termina solo (loop de atender peticiones). El host
    // mata el proceso al acabar la secuencia; en uso manual, Ctrl-C.
    let _ = rt.block_on(servidor);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cabecera de un módulo core: `\0asm` + versión 1.
    const CABECERA_MODULO_CORE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    /// Cabecera de un componente: `\0asm` + versión 13, layer 1.
    const CABECERA_COMPONENTE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];

    #[test]
    fn modulo_core_se_diagnostica_como_tal() {
        let diag = diagnostica_no_componente(&CABECERA_MODULO_CORE).expect("debe diagnosticar");
        assert!(diag.contains("módulo core"), "{diag}");
        assert!(diag.contains("cargo component build"), "{diag}");
    }

    #[test]
    fn componente_no_dispara_el_diagnostico() {
        assert_eq!(diagnostica_no_componente(&CABECERA_COMPONENTE), None);
    }

    /// Un componente roto **más allá** de la cabecera no es asunto de este
    /// diagnóstico: ahí el mensaje de wasmtime sí es el bueno.
    #[test]
    fn componente_con_cuerpo_basura_lo_diagnostica_wasmtime() {
        let mut bytes = CABECERA_COMPONENTE.to_vec();
        bytes.extend_from_slice(&[0xff; 16]);
        assert_eq!(diagnostica_no_componente(&bytes), None);
    }

    #[test]
    fn fichero_que_no_es_wasm_se_diagnostica() {
        let diag =
            diagnostica_no_componente(b"nombre: no soy un wasm\n").expect("debe diagnosticar");
        assert!(diag.contains("no es un fichero WebAssembly"), "{diag}");
    }

    #[test]
    fn fichero_mas_corto_que_la_cabecera_se_diagnostica() {
        let diag = diagnostica_no_componente(&CABECERA_COMPONENTE[..4]).expect("debe diagnosticar");
        assert!(diag.contains("no es un fichero WebAssembly"), "{diag}");
    }
}

/// Un `Named` del WIT al `Valor` del protobuf (la salida del componente).
fn nombrado_a_proto(n: &Named) -> pb::Value {
    use exports::anvil::step::step::Value as ValueWit;
    let dato = match &n.value {
        ValueWit::Number(x) => pb::value::Value::Number(*x),
        ValueWit::Text(s) => pb::value::Value::Text(s.clone()),
        ValueWit::Boolean(b) => pb::value::Value::Boolean(*b),
    };
    pb::Value {
        name: n.name.clone(),
        value: Some(dato),
    }
}

/// Un `Valor` del protobuf al `Named` del WIT (el parámetro de entrada).
///
/// `Err(nombre)` si el `oneof` llegó vacío: no hay forma de construir el
/// `variant` del WIT sin saber de qué tipo es, y no la hay a propósito — el
/// WIT no tiene rama «desconocido», que es lo que hace imposible pasarle al
/// componente un parámetro que nadie sabe interpretar.
fn proto_a_nombrado(v: &pb::Value) -> Result<Named, String> {
    use exports::anvil::step::step::Value as ValueWit;
    let valor = match v.value.as_ref().ok_or_else(|| v.name.clone())? {
        pb::value::Value::Number(x) => ValueWit::Number(*x),
        pb::value::Value::Text(s) => ValueWit::Text(s.clone()),
        pb::value::Value::Boolean(b) => ValueWit::Boolean(*b),
    };
    Ok(Named {
        name: v.name.clone(),
        value: valor,
    })
}

#[cfg(test)]
mod tests_contrato {
    use super::*;

    /// La red que sostiene el `const CONTRACT` copiado a mano. Ver su
    /// comentario: el puente no puede linkar `modelo` en el binario, así que
    /// la única forma de que las dos copias no se separen es compararlas en
    /// un test.
    #[test]
    fn el_contrato_del_puente_es_el_de_modelo() {
        assert_eq!(
            CONTRACT,
            modelo::proto::CONTRACT,
            "el puente responde el eco por los componentes WASM: si su número \
             se queda atrás, dice que entiende un contrato que no entiende"
        );
    }

    #[test]
    fn un_parametro_sin_tipo_no_llega_al_componente() {
        // Regla 2 de ADR-0019 en la frontera del puente. Si esto empezara a
        // devolver `Ok`, un paso mediría sin un parámetro que le mandaron.
        let v = pb::Value {
            name: "channel".into(),
            value: None,
        };
        assert_eq!(proto_a_nombrado(&v).unwrap_err(), "channel");
    }

    #[test]
    fn los_tres_tipos_cruzan_la_frontera_en_los_dos_sentidos() {
        use exports::anvil::step::step::Value as ValueWit;
        for wit in [
            ValueWit::Number(4.2),
            ValueWit::Text("banco-3".into()),
            ValueWit::Boolean(true),
        ] {
            let n = Named {
                name: "p".into(),
                value: wit,
            };
            let ida = nombrado_a_proto(&n);
            let vuelta = proto_a_nombrado(&ida).expect("un valor con tipo siempre vuelve");
            assert_eq!(vuelta.name, "p");
            assert_eq!(nombrado_a_proto(&vuelta), ida);
        }
    }
}
