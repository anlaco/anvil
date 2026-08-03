//! Host nativo de Anvil (ADR-0011): un único binario que **hospeda wasmtime
//! como librería** y orquesta los dos guests WASM — `anvil-guest.wasm` (motor)
//! y `ejecutor_pasos.wasm` (ejecutor) — embebidos en el propio binario.
//!
//! El usuario descarga un binario y corre:
//!
//! ```sh
//! ./anvil <secuencia.yaml> [--json <ruta>] [--csv <ruta>] [--limits <ruta>]
//! ```
//!
//! El host no parsea la línea de comandos (salvo `--solo-loopback`, que es
//! suyo): la pasa al guest motor (`inherit_args`), que la parsea como hoy.
//! El host sólo:
//!  1. Lee el YAML de la secuencia (M5-ext.1/2) para recolectar los
//!     `ejecutores:` — las IPs no-loopback declaradas (relajación acotada
//!     del loopback de ADR-0011: sólo se permiten las declaradas) y, en
//!     M5-ext.2, los `.wasm` por path a cargar.
//!  2. Arranca el ejecutor (thread) que bindea `127.0.0.1:9100` en su sandbox
//!     (loopback-only, sin relajación).
//!  3. Espera a que escuche (un `connect` de prueba; el ejecutor lo descarta).
//!  4. **M5-ext.2:** instancia cada ejecutor `tipo: wasm` del YAML (un
//!     `Store` por path, sandbox loopback-only, puerto efímero `ANVIL_PORT`)
//!     y lo expone al motor como un override `--ejecutor` sintético (M5-ext.1
//!     convierte `wasm` → `grpc` al aplicarlo). Espera a cada uno (readiness).
//!  5. Arranca el motor (main) cuyo sandbox permite loopback **más** las IPs
//!     no-loopback declaradas en `ejecutores:` (sólo ésas). Conecta, corre la
//!     secuencia y sale.
//!  6. Propaga el exit del motor. Los threads de los ejecutores se abortan al
//!     acabar el proceso.
//!
//! Los guests hablan por gRPC sobre **loopback TCP** restringido
//! (`socket_addr_check → is_loopback`), salvo las IPs no-loopback
//! declaradas (ADR-0011, relajación acotada). El sandbox WASM y el
//! aislamiento motor↔ejecutor (un `Store` por guest) se preservan.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// Guests embebidos (construidos a `wasm32-wasip2` y copiados a `OUT_DIR` por
/// `build.rs`).
const ANVIL_GUEST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anvil-guest.wasm"));
const EJECUTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ejecutor_pasos.wasm"));

const PUERTO: u16 = 9100;

/// Estado de cada guest: el contexto WASI (sockets/preopen/args) + la tabla
/// de recursos que `wasmtime-wasi` necesita.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

/// `WasiCtxBuilder` base con stdio heredado y **sockets restringidos a
/// loopback** (sólo `127.0.0.0/8` y `::1`). `inherit_network` da acceso a la
/// red del host; `socket_addr_check` rechaza cualquier IP no-loopback.
fn wasi_loopback() -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(|addr, _| Box::pin(async move { addr.ip().is_loopback() }));
    b
}

/// Como `wasi_loopback`, pero **relajando el loopback de forma acotada**
/// (ADR-0011, M5-ext.1): además de loopback, se permiten exactamente las
/// IPs no-loopback declaradas en `ejecutores:` del YAML. Sin declaración,
/// el comportamiento es idéntico a `wasi_loopback` (loopback-only).
fn wasi_loopback_con_declaradas(ips_declaradas: HashSet<IpAddr>) -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(move |addr, _| {
        let permitido = addr.ip().is_loopback() || ips_declaradas.contains(&addr.ip());
        Box::pin(async move { permitido })
    });
    b
}

/// IPs no-loopback declaradas en `ejecutores:` del YAML de la secuencia
/// (sólo `tipo: grpc` con `host` no-loopback). Es la "declaración" que
/// justifica la relajación del loopback de ADR-0011: nada sale de loopback
/// sin declararlo. Hosts que no parsean como IP (p. ej. `localhost`) no se
/// incluyen (el motor ya fallará al conectar; el sandbox no las deja pasar).
fn ips_no_loopback_declaradas(programa: &modelo::Programa) -> HashSet<IpAddr> {
    programa
        .ejecutores
        .values()
        .filter_map(|def| match &def.tipo {
            modelo::TipoEjecutor::Grpc { host, .. } => host
                .parse::<IpAddr>()
                .ok()
                .filter(|ip| !ip.is_loopback()),
            _ => None,
        })
        .collect()
}

/// Instancia y ejecuta un guest (componente WASI P2 `wasi:cli/run`) en su
/// propio `Store`. Devuelve el resultado de `call_run` para que el llamador
/// decida el exit. `bytes` = el `.wasm` embebido.
fn correr_guest(
    engine: &Engine,
    wasi: WasiCtx,
    bytes: &[u8],
) -> wasmtime::Result<Result<(), ()>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    let state = State { wasi, table: ResourceTable::new() };
    let mut store = Store::new(engine, state);
    let component = Component::from_binary(engine, bytes)?;
    let command = wasmtime_wasi::p2::bindings::sync::Command::instantiate(&mut store, &component, &linker)?;
    command.wasi_cli_run().call_run(&mut store)
}

/// Un ejecutor `.wasm` cargado por path (M5-ext.2, ADR-0014): el host lo
/// instancia en un `Store` propio (sandbox loopback-only), le asigna un
/// **puerto efímero** de loopback vía env `ANVIL_PORT`, y lo corre en un
/// thread (detached: se aborta al salir el proceso, como el ejecutor
/// embebido). El guest habla `paso.proto` por gRPC, igual que el embebido.
///
/// `puerto` es el asignado (para el readiness y para exponerlo al motor).
/// Un `.wasm` por path = un Store; dos ejecutores que comparten path
/// comparten Store (la deduplicación la hace el llamador).
struct EjecutorWasm {
    nombre: String,
    path: String,
    puerto: u16,
}

/// Carga un `.wasm` por path en un `Store` propio con sandbox loopback-only
/// (sólo recibe del motor, nunca de la red exterior), le inyecta `ANVIL_PORT`
/// con un **puerto efímero** (`bind 127.0.0.1:0`), y lo arranca en un thread.
///
/// El puerto efímero lo reserva el host (bind+drop antes de lanzar el guest):
/// el guest bindea `127.0.0.1:<puerto>` él mismo (convención `ANVIL_PORT`,
/// ADR-0014). Hay una ventana mínima entre el drop y el bind del guest; es la
/// misma técnica que ya usa `esperar_ejecutor` y suficiente para MVP.
fn instanciar_wasm(engine: &Engine, nombre: &str, path: &Path) -> Result<EjecutorWasm, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("el ejecutor '{nombre}' ({}) no se pudo leer: {e}", path.display()))?;

    // Reservar un puerto efímero de loopback para el guest.
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("no se pudo reservar puerto para el ejecutor '{nombre}': {e}"))?;
    let puerto = listener.local_addr().map_err(|e| {
        format!("no se pudo leer el puerto reservado para el ejecutor '{nombre}': {e}")
    })?.port();
    drop(listener);

    // Sandbox: loopback-only, sin relajación (sólo atiende al motor).
    let mut wasi = wasi_loopback();
    wasi.env("ANVIL_PORT", &puerto.to_string());
    let wasi = wasi.build();

    let engine = engine.clone();
    let nombre_thread = format!("ejecutor-wasm-{nombre}");
    let _ = thread::Builder::new()
        .name(nombre_thread)
        .spawn(move || {
            let _ = correr_guest(&engine, wasi, &bytes);
        })
        .map_err(|e| format!("no se pudo lanzar el thread del ejecutor '{nombre}': {e}"))?;

    Ok(EjecutorWasm { nombre: nombre.into(), path: path.display().to_string(), puerto })
}

/// Espera a que un ejecutor `.wasm` escuche en su puerto asignado (mismo
/// patrón polling que `esperar_ejecutor`). Timeout agregado por módulo;
/// falla con un mensaje claro nombrando al ejecutor.
fn esperar_wasm(exec: &EjecutorWasm) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", exec.puerto);
    for _ in 0..500 {
        if let Ok(c) = TcpStream::connect(&addr) {
            drop(c);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!(
        "el ejecutor '{}' ({}) no empezó a escuchar en {addr}",
        exec.nombre, exec.path
    ))
}

/// Espera a que el ejecutor escuche en `127.0.0.1:PUERTO` con un `connect`
/// de prueba. El ejecutor (loop de aceptar) descarta esa conexión.
fn esperar_ejecutor() {
    let addr = format!("127.0.0.1:{PUERTO}");
    for _ in 0..500 {
        if let Ok(c) = TcpStream::connect(&addr) {
            drop(c); // conexión de prueba: se cierra; el ejecutor la descarta.
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    eprintln!("el ejecutor de pasos no empezó a escuchar en {addr}");
    std::process::exit(1);
}

fn main() {
    // El host parsea un único flag propio: `--solo-loopback` (rechaza
    // cualquier `grpc` no-loopback declarado, para CI/paranoia). El resto de
    // la línea de comandos se pasa tal cual al guest motor.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let solo_loopback = args.iter().any(|a| a == "--solo-loopback");
    let args_motor: Vec<String> =
        args.iter().filter(|a| *a != "--solo-loopback").cloned().collect();

    // M5-ext.1/2: leer el YAML para recolectar los `ejecutores:` declarados.
    // El primer argumento del motor es la ruta de la secuencia (si falta, el
    // motor ya se queja; aquí no hacemos nada especial).
    let mut ips_no_loopback: HashSet<IpAddr> = HashSet::new();
    let mut programa: Option<modelo::Programa> = None;
    if let Some(ruta) = args_motor.first() {
        match cargador::cargar_programa_de_archivo(ruta) {
            Ok(p) => {
                ips_no_loopback = ips_no_loopback_declaradas(&p);
                if solo_loopback && !ips_no_loopback.is_empty() {
                    let lista: Vec<String> = ips_no_loopback.iter().map(|i| i.to_string()).collect();
                    eprintln!(
                        "--solo-loopback: la secuencia declara ejecutores en IPs no-loopback ({})",
                        lista.join(", ")
                    );
                    std::process::exit(1);
                }
                programa = Some(p);
            }
            Err(e) => {
                eprintln!("aviso: no se pudo leer '{ruta}' para los ejecutores: {e}");
            }
        }
    }

    let engine = Engine::default();

    // --- M5-ext.2: instanciar los ejecutores `tipo: wasm` declarados en el
    // --- YAML y exponerlos al motor como overrides `--ejecutor` sintéticos.
    // --- El guest motor re-parsea el YAML él mismo (ADR-0005: el motor no
    // --- recibe un `Programa` en memoria), así que el host no puede
    // --- reescribirle el modelo: compone `--ejecutor nombre=127.0.0.1:puerto`
    // --- (M5-ext.1, que ya convierte `wasm` → `grpc` al aplicarlo).
    let ruta_yaml = args_motor.first().cloned().unwrap_or_default();
    let dir_yaml = Path::new(&ruta_yaml).parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut ejecutores_wasm: Vec<EjecutorWasm> = Vec::new();
    let mut overrides_motor: Vec<String> = Vec::new();
    let mut args_motor_final: Vec<String> = args_motor;
    if let Some(p) = programa.as_ref() {
        // Deduplicar por path (dos ejecutores con el mismo `.wasm` → un Store).
        let mut stores_por_path: HashMap<String, u16> = HashMap::new();
        let mut errores: Vec<String> = Vec::new();
        for (nombre, def) in &p.ejecutores {
            if let modelo::TipoEjecutor::Wasm { path } = &def.tipo {
                // El cargador ya validó que el path existe (fail-fast al
                // cargar); aquí lo resolvemos relativo al directorio del YAML.
                let ruta = cargador::normalizar_path(&dir_yaml, Path::new(path));
                let clave = ruta.to_string_lossy().into_owned();
                let puerto = if let Some(puerto) = stores_por_path.get(&clave) {
                    *puerto
                } else {
                    let exec = match instanciar_wasm(&engine, nombre, &ruta) {
                        Ok(e) => e,
                        Err(e) => {
                            errores.push(e);
                            continue;
                        }
                    };
                    let puerto = exec.puerto;
                    if let Err(e) = esperar_wasm(&exec) {
                        errores.push(e);
                        continue;
                    }
                    eprintln!("ejecutor '{}' cargado ({} → 127.0.0.1:{})", nombre, ruta.display(), puerto);
                    ejecutores_wasm.push(exec);
                    stores_por_path.insert(clave, puerto);
                    puerto
                };
                overrides_motor.push(format!("{nombre}=127.0.0.1:{puerto}"));
            }
        }
        if !errores.is_empty() {
            for e in &errores {
                eprintln!("{e}");
            }
            std::process::exit(1);
        }
    }
    for o in &overrides_motor {
        args_motor_final.push("--ejecutor".into());
        args_motor_final.push(o.clone());
    }

    // --- Thread ejecutor: bind 127.0.0.1:9100 en su sandbox. ---
    // El ejecutor embebido sigue loopback-only (no atiende IPs externas).
    let exec_engine = engine.clone();
    let exec_handle = thread::spawn(move || {
        let wasi = wasi_loopback().build();
        // El ejecutor loop infinito de aceptar; su resultado se ignora.
        let _ = correr_guest(&exec_engine, wasi, EJECUTOR);
    });

    // --- Esperar a que escuche antes de lanzar el motor (que no reintenta). ---
    esperar_ejecutor();

    // --- Motor: hereda los args del host (secuencia + flags), preopen cwd.
    // --- Su sandbox permite loopback + las IPs no-loopback declaradas.
    let mut wasi = wasi_loopback_con_declaradas(ips_no_loopback);
    // argv[0] es el nombre del comando (el guest motor hace `args().skip(1)`).
    let mut argv: Vec<String> = vec!["anvil".to_string()];
    argv.extend(args_motor_final);
    wasi.args(&argv);
    if let Ok(cwd) = std::env::current_dir() {
        let _ = wasi.preopened_dir(&cwd, ".", DirPerms::all(), FilePerms::all());
    }
    let wasi = wasi.build();

    // El motor corre en el thread principal: su exit determina el del host.
    let r = correr_guest(&engine, wasi, ANVIL_GUEST);

    // El std de Rust en `wasm32-wasip2` normaliza `process::exit(non-zero)`
    // a `I32Exit(1)` (pérdida del código exacto, conocido en WASI P2). Así que
    // aquí propagamos 0 en éxito y el código del `I32Exit` (típicamente 1) en
    // fallo. El mensaje de error del motor va a stderr y guía al usuario.
    let exit_code = match r {
        Ok(Ok(())) => 0,
        Ok(Err(())) => 1,
        Err(e) => {
            if let Some(code) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                code.0
            } else {
                eprintln!("el motor falló: {e}");
                1
            }
        }
    };

    // Los threads de los ejecutores (loops infinitos de aceptar) se abortan
    // al salir. Los `.wasm` cargados quedan en el proceso hasta aquí (preload,
    // como TestStand por defecto); no hay shutdown ordenado en el MVP.
    drop(exec_handle);
    drop(ejecutores_wasm);
    std::process::exit(exit_code);
}
