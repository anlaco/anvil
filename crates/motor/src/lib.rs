//! Motor genérico de secuencias: lee una `DefinicionSecuencia` (datos) y la
//! corre invocando cada paso **por gRPC**, nunca con una llamada directa.
//!
//! Que todo paso pase por el cable es deliberado y es la decisión de
//! arquitectura del proyecto: aísla cada paso y deja la puerta abierta a
//! pasos escritos en cualquier lenguaje. El coste de una llamada local es
//! irrelevante frente al tiempo de un instrumento real.
//!
//! Desde M4: la secuencia puede declarar **expresiones** (precondición,
//! asigna, statement) y **control de flujo** (disable, pause_on_fail) que el
//! motor evalúa contra su entorno de variables (RF-31/33/34/35). El motor
//! sigue sin conocer el dominio del paso (ADR-0005): las expresiones son
//! datos, y el contrato `paso.proto` no cambia (ADR-0008, extendido por
//! ADR-0009).

mod entorno;

use modelo::proto::{PeticionPaso, ResultadoPasoProto, RUTA_INVOCA};
use modelo::{
    Asignacion, DefinicionEjecutor, DefinicionPaso, DefinicionSecuencia, Fase, Limite, Programa,
    ResultSink, ResultadoSecuencia, ResultadoStep, TipoEjecutor, TipoPaso,
};
use prost::Message;
use wasi_grpc::grpc::Cliente;
use wasi_grpc::net;

pub use entorno::EntornoMotor;
use expr::{eval, eval_sentencias, Entorno, Expresion, Scope, Sentencia, Value};
use std::collections::HashMap;

/// Clave interna de la conexión al ejecutor embebido en `Motor.conexiones`.
/// No es declarable en el YAML: el cargador rechaza un ejecutor con este
/// nombre (ver `cargador::NOMBRE_EMBEDIDO_RESERVADO`).
pub const EMBEDIDO: &str = "__anvil_embebido__";

/// El motor: un cliente gRPC contra los ejecutores de pasos. M5-ext.1
/// (RF-36.3): despacha cada paso al endpoint del ejecutor que declara su
/// `DefinicionPaso.ejecutor`; sin declaración, va al embebido (`EMBEDIDO`).
/// La tabla `conexiones` se abre en `desde_programa` y cada `Grpc`
/// declarado tiene su `Cliente` propio.
pub struct Motor {
    /// Conexiones abiertas, keyed por nombre de ejecutor (o `EMBEDIDO`
    /// para el embebido). Un `TipoEjecutor::Wasm` **no** abre conexión: el
    /// motor nunca lo ejecuta (ADR-0014) — el host lo traduce a `grpc`
    /// (override `--ejecutor`) antes de que llegue aquí; si llega sin
    /// traducir, `Error::EjecutorWasmSinHost`.
    conexiones: HashMap<String, Cliente>,
}

/// Qué salió mal al correr una secuencia. Un paso que *falla* no es un
/// error del motor — eso es un resultado válido; esto es que la
/// comunicación se rompió o el routing de ejecutores falló.
#[derive(Debug)]
pub enum Error {
    Red(net::Error),
    Protobuf(prost::DecodeError),
    /// El paso declara `ejecutor: <nombre>` pero ese ejecutor no está en
    /// `Programa.ejecutores`. El cargador ya lo rechaza al cargar; esto es
    /// defense in depth.
    EjecutorNoDeclarado(String),
    /// El ejecutor del paso no tiene conexión abierta (debería abrirse en
    /// `desde_programa` para los `grpc`; el embebido siempre la tiene).
    EjecutorNoConectado(String),
    /// El paso declara `ejecutor: <nombre>` con `tipo: wasm` y llegó al motor
    /// **sin traducir**. Eso sólo pasa si se corre el guest motor suelto
    /// (`wasmtime run anvil.wasm`) sin el host: el cargador de `.wasm` por
    /// path vive en el host (M5-ext.2, ADR-0014), que lo instancia y lo
    /// expone como `grpc` (override `--ejecutor`). El motor no ejecuta
    /// `Wasm` nunca.
    EjecutorWasmSinHost(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Red(e) => write!(f, "{e}"),
            Error::Protobuf(e) => write!(f, "respuesta ilegible: {e}"),
            Error::EjecutorNoDeclarado(n) => {
                write!(f, "el ejecutor '{n}' no está declarado en 'ejecutores:'")
            }
            Error::EjecutorNoConectado(n) => {
                write!(f, "el ejecutor '{n}' no tiene conexión abierta")
            }
            Error::EjecutorWasmSinHost(n) => write!(
                f,
                "el ejecutor '{n}' es 'wasm': el cargador de `.wasm` por path vive en \
                 anvil-host (M5-ext.2); corre con `./anvil <secuencia.yaml>` en vez de \
                 `wasmtime run anvil.wasm`"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<net::Error> for Error {
    fn from(e: net::Error) -> Self {
        Error::Red(e)
    }
}

impl From<prost::DecodeError> for Error {
    fn from(e: prost::DecodeError) -> Self {
        Error::Protobuf(e)
    }
}

impl Motor {
    /// Conecta al ejecutor embebido (`127.0.0.1:9100`) — compat con M4b
    /// (una secuencia sin `ejecutores:` se corre entera contra él). Es lo
    /// que usa `ejecuta_secuencia` (legacy).
    pub fn conecta(host: &str, puerto: u16) -> Result<Self, Error> {
        let mut conexiones = HashMap::new();
        conexiones.insert(EMBEDIDO.into(), Cliente::conectar(host, puerto)?);
        Ok(Motor { conexiones })
    }

    /// Conecta al embebido y abre una conexión por cada ejecutor `grpc`
    /// declarado en `Programa.ejecutores` (M5-ext.1, RF-36.3). Un ejecutor
    /// `wasm` **no** abre conexión aquí (M5-ext.1 no lo instancia); un
    /// `embebido` declarado explícitamente usa la conexión `EMBEDIDO`.
    pub fn desde_programa(programa: &Programa) -> Result<Self, Error> {
        Self::desde_programa_en(programa, "127.0.0.1", 9100)
    }

    /// Igual que `desde_programa`, con el endpoint del **embebido** explícito:
    /// lo necesita el CLI para honrar `--port` (RF-40) y reintentar la
    /// conexión mientras el ejecutor arranca.
    pub fn desde_programa_en(programa: &Programa, host: &str, puerto: u16) -> Result<Self, Error> {
        let mut motor = Self::conecta(host, puerto)?;
        for (nombre, def) in &programa.ejecutores {
            if let TipoEjecutor::Grpc { host, puerto } = &def.tipo {
                motor
                    .conexiones
                    .insert(nombre.clone(), Cliente::conectar(host, *puerto)?);
            }
        }
        Ok(motor)
    }

    /// Resuelve el endpoint de un paso (M5-ext.1, RF-36.3): sin `ejecutor`
    /// declarado → embebido; `Embebido` declarado → embebido; `Grpc` →
    /// su nombre (clave de `conexiones`); `Wasm` → error (M5-ext.2: el motor
    /// no ejecuta `Wasm`; el host lo traduce a `grpc` antes de llegar aquí).
    fn resolver_endpoint<'a>(
        def: &'a DefinicionPaso,
        programa: &'a Programa,
    ) -> Result<&'a str, Error> {
        match def.ejecutor.as_deref() {
            None => Ok(EMBEDIDO),
            Some(nombre) => match programa.ejecutores.get(nombre) {
                Some(DefinicionEjecutor {
                    tipo: TipoEjecutor::Embebido,
                    ..
                }) => Ok(EMBEDIDO),
                Some(DefinicionEjecutor {
                    tipo: TipoEjecutor::Grpc { .. },
                    ..
                }) => Ok(nombre),
                Some(DefinicionEjecutor {
                    tipo: TipoEjecutor::Wasm { .. },
                    ..
                }) => Err(Error::EjecutorWasmSinHost(nombre.to_string())),
                None => Err(Error::EjecutorNoDeclarado(nombre.to_string())),
            },
        }
    }

    /// Invoca un paso por nombre contra el endpoint de su ejecutor. Cada
    /// llamada gasta un stream HTTP/2 nuevo; de eso se encarga el cliente
    /// de `wasi-grpc`.
    fn ejecuta_paso(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
        intento: i32,
    ) -> Result<ResultadoStep, Error> {
        let endpoint = Self::resolver_endpoint(def, programa)?;
        let peticion = PeticionPaso {
            nombre: def.nombre.clone(),
            intento,
        };
        let cliente = self
            .conexiones
            .get_mut(endpoint)
            .ok_or_else(|| Error::EjecutorNoConectado(endpoint.to_string()))?;
        let bytes = cliente.unaria(RUTA_INVOCA, &peticion.encode_to_vec())?;
        Ok(ResultadoPasoProto::decode(&bytes[..])?.into())
    }

    /// Corre un paso hasta que pase o se agoten los intentos.
    /// `reintentos` es el número **total** de intentos: 1 = un solo tiro.
    fn ejecuta_con_reintentos(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
    ) -> Result<ResultadoStep, Error> {
        let max = def.reintentos.max(1);
        let mut resultado = self.ejecuta_paso(def, programa, 1)?;
        let mut intento = 1;
        while !resultado.paso() && intento < max {
            intento += 1;
            resultado = self.ejecuta_paso(def, programa, intento as i32)?;
        }
        // El límite (si la secuencia lo declara) se evalúa tras la invocación:
        // el paso devuelve la medida, el motor produce el estado final
        // (ADR-0008). El contrato `paso.proto` no cambia — el límite vive en
        // la definición, no en el cable.
        Ok(aplicar_limite(def, resultado))
    }

    /// Corre una secuencia completa y vierte el resultado a `sink` a medida
    /// que avanza. La semántica es la de la spec y no cambia:
    ///
    /// - **Setup**: corren todos; si alguno no pasa, el Main se salta entero.
    /// - **Main**: solo si el Setup fue bien, y **corta en el primer fallo**.
    /// - **Cleanup**: corre siempre, pase lo que pase antes.
    ///
    /// Desde M4: cada paso puede declarar `disable` (se salta sin invocarse),
    /// `precondicion` (se salta si es falsa, sin gastar intento), `pause_on_fail`
    /// (detiene la fase en curso al fallar — en Setup corta; Main ya corta;
    /// Cleanup nunca corta, respeta el principio "siempre") y ser un paso
    /// `statement` (local, sin gRPC). Tras un paso `Grpc` con `asigna`, vuelca
    /// campos de `resultado` a `Locals`.
    ///
    /// El motor dispara el lifecycle del `ResultSink` (`on_inicio_secuencia`
    /// → `on_inicio_paso`/`on_resultado`/`on_fin_paso` por paso →
    /// `on_fin_secuencia`), **incluido los pasos saltados** (estado
    /// `"saltado"`), para que el lifecycle sea uniforme.
    ///
    /// Si `ejecuta_con_reintentos` propaga un `Error` (red rota), la
    /// secuencia se interrumpe y **no** se dispara `on_fin_paso` ni
    /// `on_fin_secuencia` del paso en curso: el lifecycle completo solo se
    /// garantiza si la secuencia no se interrumpe por error de red.
    pub fn ejecuta_secuencia(
        &mut self,
        definicion: &DefinicionSecuencia,
        sink: &mut impl ResultSink,
    ) -> Result<ResultadoSecuencia, Error> {
        // API legacy (M1–M4): una secuencia sin subsecuencias. Construye un
        // `Programa` trivial y delega en `ejecuta_secuencia_interna`.
        let programa = Programa {
            raiz: definicion.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let (secuencia, _) =
            ejecuta_secuencia_interna(self, &programa.raiz, entorno, sink, &programa, 0, true)?;
        Ok(secuencia)
    }

    /// Ejecuta un **programa** (M4b, RF-27): la secuencia raíz, con sus
    /// `sequence_call` resueltos a subsecuencias inline (por nombre) o a
    /// archivos externos (por path, ya cargados en `programa.archivos`).
    /// El motor **no** abre ficheros: todo vino resuelto del cargador
    /// (ADR-0005). El render lo hacen los sinks de formato en
    /// `on_fin_secuencia`, que aquí sí se dispara (es la raíz).
    pub fn ejecuta_programa(
        &mut self,
        programa: &Programa,
        sink: &mut impl ResultSink,
    ) -> Result<ResultadoSecuencia, Error> {
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let (secuencia, _) =
            ejecuta_secuencia_interna(self, &programa.raiz, entorno, sink, programa, 0, true)?;
        Ok(secuencia)
    }
}

/// Cómo se invoca un paso `Grpc`: el único punto de contacto con la red.
/// `Motor` lo implementa contra gRPC; los tests del motor usan un mock (sin
/// red) y así pueden probar el flujo completo —incluido sequence call— sin
/// levantar un ejecutor. Es la materialización de "motor genérico"
/// (ADR-0005): la lógica de la secuencia no sabe si el paso corre por gRPC o
/// por un sustituto.
pub trait InvocaPasos {
    fn ejecuta_paso_grpc(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
    ) -> Result<ResultadoStep, Error>;
}

impl InvocaPasos for Motor {
    fn ejecuta_paso_grpc(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
    ) -> Result<ResultadoStep, Error> {
        self.ejecuta_con_reintentos(def, programa)
    }
}

/// Profundidad máxima de anidamiento de sequence calls (red de seguridad
/// ante un ciclo que escapara a la detección del cargador).
const PROFUNDIDAD_MAX: usize = 64;

/// Núcleo recursivo compartido por la raíz y las subsecuencias. Devuelve
/// el `ResultadoSecuencia` **y** el `EntornoMotor` final (para que un
/// sequence call padre extraiga los `parameters` finales y los copie de
/// vuelta por by-reference).
///
/// `es_raiz` distingue la raíz de una subsecuencia: sólo la raíz dispara
/// `on_inicio/on_fin_secuencia` (los sinks de formato renderizan ahí, así
/// no duplican reporte por subsecuencia). Los hooks de paso se disparan
/// siempre, al sink real, para que un futuro sink de streaming vea la
/// subsecuencia en vivo. `profundidad` acota el anidamiento (64, red de
/// seguridad ante un ciclo que escapara al cargador).
fn ejecuta_secuencia_interna<I: InvocaPasos>(
    inv: &mut I,
    def: &DefinicionSecuencia,
    mut entorno: EntornoMotor,
    sink: &mut impl ResultSink,
    programa: &Programa,
    profundidad: usize,
    es_raiz: bool,
) -> Result<(ResultadoSecuencia, EntornoMotor), Error> {
    if es_raiz {
        sink.on_inicio_secuencia(def);
    }
    let mut secuencia = ResultadoSecuencia::nueva(&def.nombre);
    // El contexto sólo cambia de fase entre secciones.
    let ctx = |fase| Contexto {
        def_en_curso: def,
        programa,
        profundidad,
        fase,
    };

    // --- Setup: corren todos. Un saltado no estropea el setup. ---
    let mut setup_ok = true;
    for p in &def.pasos_setup {
        let r = corre_un_paso(inv, p, &mut entorno, sink, &ctx(Fase::Setup))?;
        let fallo = !r.paso() && r.estado != "saltado";
        secuencia.registra(r.clone());
        if fallo {
            setup_ok = false;
        }
        if p.pause_on_fail && fallo {
            break;
        }
    }

    // --- Main: solo si el Setup fue bien; corta en el primer fallo. ---
    if setup_ok {
        for p in &def.pasos_main {
            let r = corre_un_paso(inv, p, &mut entorno, sink, &ctx(Fase::Main))?;
            let fallo = !r.paso() && r.estado != "saltado";
            secuencia.registra(r.clone());
            if fallo {
                break;
            }
        }
    }

    // --- Cleanup siempre. pause_on_fail NO corta el Cleanup. ---
    for p in &def.pasos_cleanup {
        let r = corre_un_paso(inv, p, &mut entorno, sink, &ctx(Fase::Cleanup))?;
        secuencia.registra(r.clone());
    }

    if es_raiz {
        sink.on_fin_secuencia(&secuencia);
    }
    Ok((secuencia, entorno))
}

/// Lo que un paso necesita saber de la corrida que lo envuelve: la secuencia
/// en curso (para resolver un `sequence_call` inline), el programa (para los
/// ejecutores y las subsecuencias externas), la profundidad de anidamiento y
/// la fase de la que viene.
struct Contexto<'a> {
    def_en_curso: &'a DefinicionSecuencia,
    programa: &'a Programa,
    profundidad: usize,
    fase: Fase,
}

/// Corre un solo paso (Setup/Main/Cleanup comparten esta lógica): disable,
/// precondición, invocación (Grpc, statement local o sequence call),
/// asigna y lifecycle del sink. Devuelve el `ResultadoStep` a registrar.
///
/// La fase del contexto se sella en el resultado **antes** de
/// `on_resultado`, para que un sink de streaming la vea ya puesta y no sólo
/// en el agregado final.
fn corre_un_paso<I: InvocaPasos>(
    inv: &mut I,
    p: &DefinicionPaso,
    ent: &mut EntornoMotor,
    sink: &mut impl ResultSink,
    ctx: &Contexto,
) -> Result<ResultadoStep, Error> {
    sink.on_inicio_paso(p);
    let sella = |mut r: ResultadoStep| {
        r.fase = ctx.fase;
        r
    };

    // (a) disable: se salta sin invocar ni evaluar nada.
    if p.disable {
        let r = sella(ResultadoStep::nuevo(&p.nombre, "saltado", "disable"));
        sink.on_resultado(&r);
        sink.on_fin_paso(p);
        return Ok(r);
    }

    // (b) precondición (RF-33): el motor evalúa ANTES de invocar. Si es
    // falsa, se salta sin gastar intento. Bool estricto: un no-bool es
    // error de definición.
    if let Some(pre) = &p.precondicion {
        match evalua_precondicion(pre, ent, &p.nombre) {
            VeredictoPre::Continua => {}
            VeredictoPre::Salta(r) => {
                let r = sella(r);
                sink.on_resultado(&r);
                sink.on_fin_paso(p);
                return Ok(r);
            }
        }
    }

    // (c) según tipo de paso (RF-27).
    let mut r = match p.tipo {
        TipoPaso::Statement => ejecuta_statement_puro(p.statement.as_deref(), &p.nombre, ent),
        TipoPaso::PassFail => evalua_pass_fail(p.condicion.as_ref(), &p.nombre, ent),
        TipoPaso::Grpc => inv.ejecuta_paso_grpc(p, ctx.programa)?,
        TipoPaso::SequenceCall => ejecuta_sequence_call(inv, p, ent, sink, ctx)?,
    };

    // (d) asigna (RF-31): tras un paso Grpc o SequenceCall, vuelca campos
    // de `resultado` a Locals. Un statement asigna dentro de su sentencia; un
    // pass_fail no produce `resultado.*` que volcar (el cargador rechaza
    // `asigna` en un pass_fail, así que aquí no hay nada que ignorar).
    if matches!(p.tipo, TipoPaso::Grpc | TipoPaso::SequenceCall) {
        if let Some(asignaciones) = &p.asigna {
            r = aplica_asigna(asignaciones, r, ent);
        }
    }

    // El `sequence_call` lleva la fase del padre; sus `sub_pasos` ya vienen
    // sellados con la suya por la ejecución de la subsecuencia.
    let r = sella(r);
    sink.on_resultado(&r);
    sink.on_fin_paso(p);
    Ok(r)
}

/// Ejecuta un paso `sequence_call` (M4b, RF-27): invoca otra secuencia
/// como un paso, **sin gRPC**, y anida su `ResultadoSecuencia` en el
/// `ResultadoStep` del call. Los `parametros` son by-reference: copia
/// `locals.X` del padre → `parameters.P` al iniciar, y `parameters.P`
/// (final) → `locals.X` al volver (como TestStand).
fn ejecuta_sequence_call<I: InvocaPasos>(
    inv: &mut I,
    p: &DefinicionPaso,
    ent: &mut EntornoMotor,
    sink: &mut impl ResultSink,
    ctx: &Contexto,
) -> Result<ResultadoStep, Error> {
    let destino = p.secuencia.as_deref().expect("validado en a_definicion");

    // (1) Profundidad: red de seguridad ante un ciclo que escapara al
    // cargador (no debería; el cargador los detecta al cargar).
    if ctx.profundidad + 1 > PROFUNDIDAD_MAX {
        return Ok(ResultadoStep::nuevo(
            &p.nombre,
            "error",
            format!(
                "sequence call '{destino}': anidamiento demasiado profundo (>{PROFUNDIDAD_MAX})"
            ),
        ));
    }

    // (2) Resolver la subsecuencia: por nombre (inline de `def_en_curso`)
    // o por path (archivo externo ya cargado en `programa.archivos`).
    // El cargador ya validó que existe al cargar; si faltara aquí, es
    // defense in depth: se registra como `"error"` del paso (no pánico,
    // no propaga `Error` de red).
    let sub = if cargador::es_path(destino) {
        match ctx.programa.archivos.get(destino) {
            Some(s) => s,
            None => {
                return Ok(ResultadoStep::nuevo(
                    &p.nombre,
                    "error",
                    format!("sequence call '{destino}': no resuelto al cargar"),
                ));
            }
        }
    } else {
        match ctx.def_en_curso.subsecuencias.get(destino) {
            Some(s) => s,
            None => {
                return Ok(ResultadoStep::nuevo(
                    &p.nombre,
                    "error",
                    format!("sequence call '{destino}': subsecuencia inline inexistente"),
                ));
            }
        }
    };

    // (3) Entrada by-reference: cada `Argumento.origen` es `Var{Locals,
    // campo}` (validado al cargar); lee `locals.campo` del padre.
    let mut argumentos = HashMap::new();
    if let Some(args) = &p.parametros {
        for a in args {
            let valor = match &a.origen {
                Expresion::Var {
                    scope: Scope::Locals,
                    campo,
                } => match ent.lee(Scope::Locals, campo) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(ResultadoStep::nuevo(
                                &p.nombre,
                                "error",
                                format!("sequence call '{destino}': argumento '{}': locals.{campo} no existe ({e})", a.param),
                            ));
                    }
                },
                // La forma ya se validó al cargar; defense in depth.
                _ => {
                    return Ok(ResultadoStep::nuevo(
                        &p.nombre,
                        "error",
                        format!(
                            "sequence call '{destino}': argumento '{}' no es locals.X",
                            a.param
                        ),
                    ));
                }
            };
            argumentos.insert(a.param.clone(), valor);
        }
    }

    // (4) Ejecuta la subsecuencia contra un entorno con `parameters`
    // mutables (by-reference) y los argumentos inyectados. No dispara
    // `on_inicio/on_fin_secuencia` (es_raiz=false): los sinks de formato
    // no duplican reporte; el árbol anidado se reconstruye en el paso (6).
    let sub_entorno = EntornoMotor::desde_definicion_con_argumentos(sub, argumentos, true);
    let (sub_secuencia, sub_entorno) = ejecuta_secuencia_interna(
        inv,
        sub,
        sub_entorno,
        sink,
        ctx.programa,
        ctx.profundidad + 1,
        false,
    )?;

    // (5) Salida by-reference: copia `parameters.P` (final) → `locals.campo`
    // del padre (el mismo lvalue de la entrada).
    let mut errores_salida = Vec::new();
    if let Some(args) = &p.parametros {
        for a in args {
            if let Expresion::Var {
                scope: Scope::Locals,
                campo,
            } = &a.origen
            {
                if let Some(v_final) = sub_entorno.parameters().get(&a.param) {
                    if let Err(e) = ent.escribe(Scope::Locals, campo, v_final.clone()) {
                        errores_salida.push(format!("salida '{}': {e}", a.param));
                    }
                }
            }
        }
    }

    // (6) Construye el `ResultadoStep` del call: estado agregado de la
    // subsecuencia + sub-pasos anidados.
    let estado_sub = sub_secuencia.estado();
    let mut r = ResultadoStep::nuevo(
        &p.nombre,
        estado_sub,
        format!("sequence call '{destino}' → {estado_sub}"),
    );
    r.sub_pasos = Some(sub_secuencia.pasos);
    if !errores_salida.is_empty() {
        // Una salida que no puede escribirse es un fallo de definición.
        r.estado = "error".into();
        r.mensaje = format!("{} ({})", r.mensaje, errores_salida.join("; "));
    }
    Ok(r)
}

/// Veredicto de la precondición: continuar o saltar (con el `ResultadoStep`
/// a registrar, ya sea `"saltado"` o `"error"`).
enum VeredictoPre {
    Continua,
    Salta(ResultadoStep),
}

/// Evalúa la precondición contra el entorno. Es **pura** (sin gRPC): la
/// prueba el motor sin levantar red. La precondición **no** ve el resultado
/// del paso (aún no corre), así que se limpia antes.
fn evalua_precondicion(pre: &Expresion, ent: &mut EntornoMotor, nombre: &str) -> VeredictoPre {
    ent.limpia_resultado();
    match eval(pre, ent) {
        Ok(Value::Bool(true)) => VeredictoPre::Continua,
        Ok(Value::Bool(false)) => VeredictoPre::Salta(ResultadoStep::nuevo(
            nombre,
            "saltado",
            "precondición falsa",
        )),
        Ok(v) => VeredictoPre::Salta(ResultadoStep::nuevo(
            nombre,
            "error",
            format!("precondición: se esperaba bool, no {}", v.tipo()),
        )),
        Err(e) => VeredictoPre::Salta(ResultadoStep::nuevo(
            nombre,
            "error",
            format!("precondición: {e}"),
        )),
    }
}

/// Ejecuta un paso `statement` (RF-27): evalúa sus sentencias contra el
/// entorno, sin gRPC. Pura (sin red). `stmts` es `None` sólo si el cargador
/// falló su validación (defense in depth).
fn ejecuta_statement_puro(
    stmts: Option<&[Sentencia]>,
    nombre: &str,
    ent: &mut EntornoMotor,
) -> ResultadoStep {
    let Some(stmts) = stmts else {
        return ResultadoStep::nuevo(nombre, "error", "statement sin sentencia");
    };
    match eval_sentencias(stmts, ent) {
        Ok(()) => ResultadoStep::nuevo(nombre, "paso", "statement ok"),
        Err(e) => ResultadoStep::nuevo(nombre, "error", format!("statement: {e}")),
    }
}

/// Evalúa un paso `pass_fail` (RF-25, ADR-0018): el **veredicto compuesto**
/// sobre variables ya pobladas. El motor evalúa la expresión declarada en el
/// YAML y produce el estado; el paso no interviene — mismo patrón que el
/// límite (ADR-0008) y la precondición (ADR-0009). Pura (sin red).
///
/// Bool **estricto**, como la precondición: un no-Bool es un fallo de
/// definición (`error`), no un `false` por truthiness. La diferencia con la
/// precondición está en el veredicto: allí un `false` **salta** el paso; aquí
/// lo **falla**, que es justo lo que faltaba para expresar un criterio de
/// aceptación que combine varias medidas (DIAG-2 del informe de beta).
///
/// `condicion` es `None` sólo si el cargador falló su validación (defense in
/// depth, como `ejecuta_statement_puro`).
fn evalua_pass_fail(
    condicion: Option<&Expresion>,
    nombre: &str,
    ent: &mut EntornoMotor,
) -> ResultadoStep {
    let Some(cond) = condicion else {
        return ResultadoStep::nuevo(nombre, "error", "pass_fail sin condición");
    };
    // Un `pass_fail` no tiene `resultado.*` propio: lee variables de scopes.
    ent.limpia_resultado();
    match eval(cond, ent) {
        Ok(Value::Bool(true)) => ResultadoStep::nuevo(nombre, "paso", "condición cumplida"),
        Ok(Value::Bool(false)) => ResultadoStep::nuevo(nombre, "fallo", "condición no cumplida"),
        Ok(v) => ResultadoStep::nuevo(
            nombre,
            "error",
            format!("condición: se esperaba bool, no {}", v.tipo()),
        ),
        Err(e) => ResultadoStep::nuevo(nombre, "error", format!("condición: {e}")),
    }
}

/// Aplica las `asignaciones` (RF-31): vuelca cada `expr` (sobre `resultado`/
/// scopes) a una Local. Pura (sin red). Si una asignación falla al evaluar o
/// al escribir, convierte el paso a `"error"` (es un fallo de definición) y
/// añade el detalle al mensaje, preservando el mensaje original del paso.
fn aplica_asigna(
    asignaciones: &[Asignacion],
    mut r: ResultadoStep,
    ent: &mut EntornoMotor,
) -> ResultadoStep {
    ent.set_resultado(r.clone());
    for a in asignaciones {
        match eval(&a.expr, ent) {
            Ok(v) => {
                if let Err(e) = ent.escribe(Scope::Locals, &a.var, v) {
                    r.estado = "error".into();
                    r.mensaje = format!("{} (asigna {}: {})", r.mensaje, a.var, e);
                }
            }
            Err(e) => {
                r.estado = "error".into();
                r.mensaje = format!("{} (asigna {}: {})", r.mensaje, a.var, e);
            }
        }
    }
    ent.limpia_resultado();
    r
}

/// Aplica el `Limite` declarado en `def` al `ResultadoStep` que devolvió el
/// paso, **tras** la invocación gRPC. Es el corazón de "los límites son datos
/// first-class" (RF-29) y de ADR-0008: el paso mide, el motor evalúa.
///
/// Reglas:
///
/// - Si `def` no lleva límite, o el paso no trae `valor_medido`, no hace nada
///   (un pass/fail o un action sin medida no se tocan).
/// - Rellena los campos de límite del `ResultadoStep` (`limite_min`/`limite_max`
///   para rango; `valor_esperado`/`operador` para comparación) para que el
///   reporte muestre el umbral aplicado.
/// - **Solo empeora `paso` → `fallo`**: si el paso ya dijo `fallo` o `error`
///   por sí mismo, se respeta (el paso es autoridad sobre su ejecución); el
///   límite es una regla de aceptación adicional, nunca una absolución.
/// - Al convertir a `fallo`, reescribe el `mensaje` al formato del límite.
///
/// Es `pub(crate)` y pura (sin gRPC ni IO) para testearla aislada.
pub(crate) fn aplicar_limite(def: &DefinicionPaso, mut r: ResultadoStep) -> ResultadoStep {
    // Sin límite declarado → el paso decide (pass/fail, action).
    let Some(lim) = &def.limite else {
        return r;
    };
    // Sin medida a la que aplicar el límite → nada que hacer (un pass/fail
    // con un límite declarado es mal uso, pero no debe pánico).
    let Some(valor) = r.valor_medido else {
        return r;
    };

    // Rellenar los campos de límite para el reporte, según el tipo.
    match lim {
        Limite::Rango { min, max } => {
            r.limite_min = Some(*min);
            r.limite_max = Some(*max);
        }
        Limite::Comparacion { op, esperado } => {
            r.operador = Some(*op);
            r.valor_esperado = Some(*esperado);
        }
    }

    // El límite solo puede empeorar un `paso` a `fallo`: nunca toca un
    // `fallo`/`error` que el paso haya emitido por sí mismo.
    if r.estado == "paso" && lim.evalua(valor) == "fallo" {
        r.estado = "fallo".into();
        r.mensaje = match lim {
            Limite::Rango { min, max } => format!("{valor} fuera de rango [{min}, {max}]"),
            Limite::Comparacion { op, esperado } => {
                format!("{valor} {} {esperado} no cumplido", op.simbolo())
            }
        };
    }
    r
}

#[cfg(test)]
mod tests {
    //! Tests de la lógica de límites del motor. `aplicar_limite` es pura, así
    //! que se prueba sin levantar gRPC: se le pasa una `DefinicionPaso` y un
    //! `ResultadoStep` construidos a mano.

    use super::*;
    use modelo::{DefinicionPaso, Limite, Operador, ResultadoStep};

    /// Un paso que midió `valor` y devuelve `estado` ("paso"/"fallo"/"error"),
    /// sin conocer el umbral: es lo que produce un paso de *limit test* en M3.
    fn paso_medido(valor: f64, estado: &str) -> ResultadoStep {
        ResultadoStep::medido_valor("medir_voltaje", estado, "medido", valor)
    }

    #[test]
    fn rango_dentro_deja_paso_y_rellena_campos() {
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(5.0, "paso"));
        assert_eq!(r.estado, "paso");
        assert_eq!(r.limite_min, Some(4.5));
        assert_eq!(r.limite_max, Some(5.5));
        assert_eq!(
            r.mensaje, "medido",
            "si pasa, el mensaje del paso se respeta"
        );
    }

    #[test]
    fn rango_fuera_convierte_paso_a_fallo_y_reescribe_mensaje() {
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(4.2, "paso"));
        assert_eq!(r.estado, "fallo");
        assert_eq!(r.limite_min, Some(4.5));
        assert_eq!(r.limite_max, Some(5.5));
        assert_eq!(r.mensaje, "4.2 fuera de rango [4.5, 5.5]");
    }

    #[test]
    fn comparacion_no_cumplida_convierte_a_fallo() {
        let def = DefinicionPaso::con_limite(
            "m",
            1,
            Limite::Comparacion {
                op: Operador::Ge,
                esperado: 1000.0,
            },
        );
        let r = aplicar_limite(&def, paso_medido(999.0, "paso"));
        assert_eq!(r.estado, "fallo");
        assert_eq!(r.operador, Some(Operador::Ge));
        assert_eq!(r.valor_esperado, Some(1000.0));
        assert_eq!(r.mensaje, "999 >= 1000 no cumplido");
    }

    #[test]
    fn el_paso_que_ya_fallo_no_se_mejora_solo_se_rellena_el_limite() {
        // El paso sabe algo que el límite no: su fallo se respeta.
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(5.0, "fallo"));
        assert_eq!(
            r.estado, "fallo",
            "el paso ya falló: el límite no lo mejora"
        );
        assert_eq!(
            r.limite_min,
            Some(4.5),
            "pero sí rellena el límite para el reporte"
        );
    }

    #[test]
    fn el_paso_con_error_no_se_toca() {
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(5.0, "error"));
        assert_eq!(r.estado, "error");
        assert_eq!(r.limite_min, Some(4.5));
    }

    #[test]
    fn paso_sin_limite_no_cambia() {
        let def = DefinicionPaso::nuevo("m", 1);
        let r = aplicar_limite(&def, paso_medido(4.2, "paso"));
        assert_eq!(r.estado, "paso");
        assert_eq!(r.limite_min, None);
    }

    #[test]
    fn paso_con_limite_pero_sin_medida_no_se_evalua() {
        // Un pass/fail con un límite declarado (mal uso) no debe pánico: sin
        // valor_medido el límite no aplica, todo se queda igual.
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, ResultadoStep::nuevo("m", "paso", "sin medida"));
        assert_eq!(r.estado, "paso");
        assert_eq!(r.limite_min, None, "sin medida no se rellena el límite");
    }

    // --- M4: precondición, statement, asigna (lógica pura, sin gRPC) ---

    use crate::entorno::EntornoMotor;
    use modelo::{DefinicionSecuencia, ValorDefinicion};

    fn entorno_con_locals(locals: &[(&str, ValorDefinicion)]) -> EntornoMotor {
        let mut def = DefinicionSecuencia::default();
        for (k, v) in locals {
            def.locals.insert((*k).to_string(), v.clone());
        }
        EntornoMotor::desde_definicion(&def)
    }

    #[test]
    fn precondicion_verdadera_continua() {
        let mut env = entorno_con_locals(&[("contador", ValorDefinicion::Numero(5.0))]);
        let pre = expr::parse_expresion("locals.contador > 0").unwrap();
        assert!(matches!(
            evalua_precondicion(&pre, &mut env, "p"),
            VeredictoPre::Continua
        ));
    }

    #[test]
    fn precondicion_falsa_salta_sin_gastar_intento() {
        let mut env = entorno_con_locals(&[("contador", ValorDefinicion::Numero(0.0))]);
        let pre = expr::parse_expresion("locals.contador > 0").unwrap();
        let r = match evalua_precondicion(&pre, &mut env, "medir") {
            VeredictoPre::Salta(r) => r,
            _ => panic!("debe saltar"),
        };
        assert_eq!(r.estado, "saltado");
        assert_eq!(r.nombre, "medir");
        assert!(r.mensaje.contains("precondición falsa"));
    }

    #[test]
    fn precondicion_no_bool_es_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(3.0))]);
        let pre = expr::parse_expresion("locals.x + 1").unwrap(); // produce número, no bool
        let r = match evalua_precondicion(&pre, &mut env, "p") {
            VeredictoPre::Salta(r) => r,
            _ => panic!("debe saltar como error"),
        };
        assert_eq!(r.estado, "error");
    }

    // --- RF-25 / ADR-0018: veredicto por expresión (pass_fail) ---

    #[test]
    fn pass_fail_condicion_verdadera_pasa() {
        let mut env = entorno_con_locals(&[("v", ValorDefinicion::Numero(5.0))]);
        let cond = expr::parse_expresion("locals.v > 4.9 && locals.v < 5.1").unwrap();
        let r = evalua_pass_fail(Some(&cond), "verificar_dut", &mut env);
        assert_eq!(r.estado, "paso");
        assert_eq!(r.nombre, "verificar_dut");
    }

    /// Lo que DIAG-2 no permitía expresar: un veredicto compuesto que **falla**.
    #[test]
    fn pass_fail_condicion_falsa_falla() {
        let mut env = entorno_con_locals(&[("v", ValorDefinicion::Numero(4.2))]);
        let cond = expr::parse_expresion("locals.v > 4.9").unwrap();
        let r = evalua_pass_fail(Some(&cond), "verificar_dut", &mut env);
        assert_eq!(r.estado, "fallo", "un veredicto falso falla el paso");
        assert!(r.mensaje.contains("condición no cumplida"));
    }

    /// Bool estricto, como la precondición: sin truthiness.
    #[test]
    fn pass_fail_no_bool_es_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(3.0))]);
        let cond = expr::parse_expresion("locals.x + 1").unwrap();
        let r = evalua_pass_fail(Some(&cond), "v", &mut env);
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("se esperaba bool"));
    }

    #[test]
    fn pass_fail_variable_inexistente_es_error() {
        let mut env = entorno_con_locals(&[]);
        let cond = expr::parse_expresion("locals.no_existe > 1").unwrap();
        let r = evalua_pass_fail(Some(&cond), "v", &mut env);
        assert_eq!(r.estado, "error");
    }

    #[test]
    fn statement_local_escribe_en_locals() {
        let mut env = entorno_con_locals(&[("ok", ValorDefinicion::Bool(true))]);
        let stmts = expr::parse_sentencias("locals.ok = false").unwrap();
        let r = ejecuta_statement_puro(Some(&stmts), "init", &mut env);
        assert_eq!(r.estado, "paso");
        assert_eq!(env.locals().get("ok"), Some(&expr::Value::Bool(false)));
    }

    #[test]
    fn statement_con_error_de_tipo_es_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(1.0))]);
        // `!locals.x` es error de tipo: x es número, `!` espera bool (sintaxis Julia).
        let stmts = expr::parse_sentencias("locals.x = !locals.x").unwrap();
        let r = ejecuta_statement_puro(Some(&stmts), "init", &mut env);
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("statement"));
    }

    #[test]
    fn asigna_vuelca_resultado_a_locals() {
        let mut env = entorno_con_locals(&[("voltaje", ValorDefinicion::Numero(0.0))]);
        let res = ResultadoStep::medido_valor("m", "paso", "ok", 4.2);
        let asignaciones = vec![modelo::Asignacion {
            var: "voltaje".into(),
            expr: expr::parse_expresion("resultado.valor_medido").unwrap(),
        }];
        let r = aplica_asigna(&asignaciones, res, &mut env);
        assert_eq!(r.estado, "paso", "el paso ya pasó; la asigna no falla");
        assert_eq!(env.locals().get("voltaje"), Some(&expr::Value::Numero(4.2)));
    }

    #[test]
    fn asigna_que_falla_convierte_el_paso_en_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(0.0))]);
        let res = ResultadoStep::medido_valor("m", "paso", "ok", 4.2);
        // `resultado.valor_medido + nulo` → error (nulo en aritmética).
        // Forzamos un nulo leyendo un campo de resultado inexistente.
        let asignaciones = vec![modelo::Asignacion {
            var: "x".into(),
            expr: expr::parse_expresion("resultado.inventado + 1").unwrap(),
        }];
        let r = aplica_asigna(&asignaciones, res, &mut env);
        assert_eq!(
            r.estado, "error",
            "una asigna que falla es un fallo de definición"
        );
        assert!(r.mensaje.contains("asigna"));
    }

    // --- M4b: sequence call (sin gRPC, con InvocadorMock) ---

    use modelo::{Programa, TipoPaso};

    /// Invocador de mentira: un paso `Grpc` **no** debería aparecer en estos
    /// tests (las subsecuencias usan `statement`). Si lo hiciera, pánico
    /// ruidoso para detectarlo.
    struct InvocadorMock;
    impl InvocaPasos for InvocadorMock {
        fn ejecuta_paso_grpc(
            &mut self,
            _def: &DefinicionPaso,
            _programa: &Programa,
        ) -> Result<ResultadoStep, Error> {
            panic!("InvocadorMock no espera pasos grpc en estos tests");
        }
    }

    /// Un sink que no hace nada (sólo nos interesa el `ResultadoSecuencia`).
    struct SinkNulo;
    impl modelo::ResultSink for SinkNulo {}

    /// Un `Argumento` by-reference: `param` ↔ `locals.campo`.
    fn arg(param: &str, campo: &str) -> modelo::Argumento {
        modelo::Argumento {
            param: param.into(),
            origen: expr::parse_expresion(&format!("locals.{campo}")).unwrap(),
        }
    }

    /// Un paso `sequence_call` hacia `destino` con los argumentos dados.
    fn call(nombre: &str, destino: &str, args: Vec<modelo::Argumento>) -> DefinicionPaso {
        let mut p = DefinicionPaso::nuevo(nombre, 1);
        p.tipo = TipoPaso::SequenceCall;
        p.secuencia = Some(destino.into());
        if !args.is_empty() {
            p.parametros = Some(args);
        }
        p
    }

    /// Una subsecuencia inline `nombre` con un `statement` que copia
    /// `parameters.canal` a `parameters.listo` (escribe en parameters: by-ref).
    fn inline_comprueba(nombre: &str) -> DefinicionSecuencia {
        let mut s = DefinicionSecuencia {
            nombre: nombre.into(),
            ..Default::default()
        };
        s.parameters
            .insert("canal".into(), ValorDefinicion::Numero(0.0));
        s.parameters
            .insert("listo".into(), ValorDefinicion::Bool(false));
        s.pasos_main = vec![{
            let mut p = DefinicionPaso::nuevo("comprobar", 1);
            p.tipo = TipoPaso::Statement;
            p.statement = Some(
                expr::parse_sentencias("parameters.listo = (parameters.canal >= 0.0)").unwrap(),
            );
            p
        }];
        s
    }

    #[test]
    fn sequence_call_inline_devuelve_estado_agregado_y_anida() {
        // Padre: locals { canal: 1.0 }, llama a inline `init` by-reference.
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre
            .locals
            .insert("listo".into(), ValorDefinicion::Bool(false));
        padre
            .subsecuencias
            .insert("init".into(), inline_comprueba("init"));
        padre.pasos_main = vec![call(
            "llamar",
            "init",
            vec![arg("canal", "canal"), arg("listo", "listo")],
        )];

        let programa = Programa {
            raiz: padre.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        // El call pasa (la subsecuencia pasa) y anida sus pasos.
        let r = &sec.pasos[0];
        assert_eq!(r.estado, "paso");
        assert_eq!(r.sub_pasos.as_ref().unwrap().len(), 1);
        assert_eq!(r.sub_pasos.as_ref().unwrap()[0].nombre, "comprobar");
    }

    #[test]
    fn sequence_call_by_reference_devuelve_la_salida_al_padre() {
        // La subsecuencia escribe `parameters.listo` = true (canal=1.0 >= 0.0);
        // al volver, by-reference copia `parameters.listo` → `locals.listo`.
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre
            .locals
            .insert("listo".into(), ValorDefinicion::Bool(false));
        padre
            .subsecuencias
            .insert("init".into(), inline_comprueba("init"));
        padre.pasos_main = vec![call(
            "llamar",
            "init",
            vec![arg("canal", "canal"), arg("listo", "listo")],
        )];

        let programa = Programa {
            raiz: padre.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, env) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        assert_eq!(sec.pasos[0].estado, "paso");
        // La salida by-reference: locals.listo pasó de false a true.
        assert_eq!(
            env.locals().get("listo"),
            Some(&Value::Bool(true)),
            "by-reference lleva la salida al padre"
        );
        // Y locals.canal sigue siendo el de entrada (la subsecuencia no lo mutó).
        assert_eq!(env.locals().get("canal"), Some(&Value::Numero(1.0)));
    }

    #[test]
    fn sequence_call_propaga_fallo_de_la_subsecuencia() {
        // canal = -1.0 → (canal >= 0.0) = false → parameters.listo = false,
        // pero la subsecuencia misma "pasa" (un statement no falla). Para
        // forzar un fallo agregado, la subsecuencia lleva un paso grpc mock
        // que falle… como el mock pánico, usamos otro camino: un paso
        // statement cuyo `estado` es "paso"; el agregado de la sub es "paso".
        // Verificamos en cambio el caso de error: una subsecuencia cuyo
        // statement tiene error de tipo.
        let mut sub = DefinicionSecuencia {
            nombre: "init".into(),
            ..Default::default()
        };
        sub.parameters
            .insert("canal".into(), ValorDefinicion::Numero(0.0));
        sub.pasos_main = vec![{
            let mut p = DefinicionPaso::nuevo("malo", 1);
            p.tipo = TipoPaso::Statement;
            // `!parameters.canal` → error de tipo (canal es número, `!` espera bool).
            p.statement =
                Some(expr::parse_sentencias("parameters.canal = !parameters.canal").unwrap());
            p
        }];
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre.subsecuencias.insert("init".into(), sub);
        padre.pasos_main = vec![call("llamar", "init", vec![arg("canal", "canal")])];

        let programa = Programa {
            raiz: padre.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        // El call hereda "error" (agregado de la sub: un statement con error).
        let r = &sec.pasos[0];
        assert_eq!(
            r.estado, "error",
            "un fallo en la subsecuencia se propaga al call"
        );
        assert_eq!(r.sub_pasos.as_ref().unwrap()[0].estado, "error");
    }

    #[test]
    fn sequence_call_subsecuencia_externa_por_path() {
        // El cargador ya reescribió `secuencia` a la clave canónica y cargó
        // el archivo en `programa.archivos`. Aquí simulamos eso a mano.
        let mut hija = DefinicionSecuencia {
            nombre: "hija".into(),
            ..Default::default()
        };
        hija.parameters
            .insert("canal".into(), ValorDefinicion::Numero(0.0));
        hija.pasos_main = vec![{
            let mut p = DefinicionPaso::nuevo("eco", 1);
            p.tipo = TipoPaso::Statement;
            p.statement =
                Some(expr::parse_sentencias("parameters.canal = parameters.canal + 1.0").unwrap());
            p
        }];
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre.pasos_main = vec![call(
            "llamar",
            "ruta/hija.yaml",
            vec![arg("canal", "canal")],
        )];

        let programa = Programa {
            raiz: padre,
            archivos: HashMap::from([("ruta/hija.yaml".to_string(), hija)]),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, env) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        assert_eq!(sec.pasos[0].estado, "paso");
        // La hija hizo canal + 1.0 = 2.0; by-reference lo devuelve a locals.canal.
        assert_eq!(env.locals().get("canal"), Some(&Value::Numero(2.0)));
    }

    #[test]
    fn sequence_call_argumento_a_local_inexistente_es_error() {
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        // No declaramos `locals.canal`: el lvalue no existe en el padre.
        padre
            .subsecuencias
            .insert("init".into(), inline_comprueba("init"));
        padre.pasos_main = vec![call("llamar", "init", vec![arg("canal", "canal")])];

        let programa = Programa {
            raiz: padre.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        // El motor registra el call como "error" (no pánico).
        assert_eq!(sec.pasos[0].estado, "error");
        assert!(sec.pasos[0].mensaje.contains("locals.canal"));
    }

    #[test]
    fn sequence_call_profundidad_excesiva_es_error() {
        // Un ciclo por path (A → B → A → …) que el cargador rechazaría al
        // cargar; aquí lo construimos a mano en el `Programa` para probar la
        // red de seguridad del motor (profundidad >64 → "error", no pánico).
        let mut a = DefinicionSecuencia {
            nombre: "a".into(),
            ..Default::default()
        };
        a.pasos_main = vec![call("a_b", "b.yaml", vec![])];
        let mut b = DefinicionSecuencia {
            nombre: "b".into(),
            ..Default::default()
        };
        b.pasos_main = vec![call("b_a", "a.yaml", vec![])];
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre.pasos_main = vec![call("p_a", "a.yaml", vec![])];

        let programa = Programa {
            raiz: padre,
            archivos: HashMap::from([("a.yaml".to_string(), a), ("b.yaml".to_string(), b)]),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        // En algún nivel la profundidad >64 produce el error de anidamiento.
        fn busca_prof(p: &ResultadoStep) -> bool {
            p.estado == "error" && p.mensaje.contains("anidamiento demasiado profundo")
                || p.sub_pasos
                    .as_ref()
                    .is_some_and(|sub| sub.iter().any(busca_prof))
        }
        assert!(
            sec.pasos.iter().any(busca_prof),
            "profundidad >64 corta con error: {:?}",
            sec.pasos
        );
    }

    #[test]
    fn subsecuencia_no_dispara_on_fin_secuencia_del_sink() {
        // Un sink que cuenta `on_fin_secuencia`: debe dispararse una sola
        // vez (la raíz), no por cada subsecuencia.
        use std::cell::Cell;
        struct Contador(Cell<u32>);
        impl modelo::ResultSink for Contador {
            fn on_fin_secuencia(&mut self, _: &ResultadoSecuencia) {
                self.0.set(self.0.get() + 1);
            }
        }
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre
            .locals
            .insert("listo".into(), ValorDefinicion::Bool(false));
        padre
            .subsecuencias
            .insert("init".into(), inline_comprueba("init"));
        padre.pasos_main = vec![call(
            "llamar",
            "init",
            vec![arg("canal", "canal"), arg("listo", "listo")],
        )];

        let programa = Programa {
            raiz: padre.clone(),
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = Contador(Cell::new(0));
        let _ = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();
        assert_eq!(
            sink.0.get(),
            1,
            "on_fin_secuencia sólo para la raíz, no por subsecuencia"
        );
    }

    // --- M5-ext.1: routing nombre→endpoint ---

    use modelo::{DefinicionEjecutor, TipoEjecutor};

    /// Invocador de mentira que devuelve un resultado distinto según el
    /// `def.ejecutor` que le llegue (el routing real lo decide el motor:
    /// `resolver_endpoint` → conexión; aquí el mock ve el `def` ya enrutado).
    struct InvocadorRuteado;
    impl InvocaPasos for InvocadorRuteado {
        fn ejecuta_paso_grpc(
            &mut self,
            def: &DefinicionPaso,
            _programa: &Programa,
        ) -> Result<ResultadoStep, Error> {
            let mensaje = match def.ejecutor.as_deref() {
                None => "embebido",
                Some(n) => n,
            };
            Ok(ResultadoStep::nuevo(&def.nombre, "paso", mensaje))
        }
    }

    /// Un `Programa` con raíz + un ejecutor `grpc` (python) y un `embebido`.
    fn programa_ruteado() -> Programa {
        let mut raiz = DefinicionSecuencia {
            nombre: "s".into(),
            ..Default::default()
        };
        let mut p1 = DefinicionPaso::nuevo("a", 1);
        p1.ejecutor = None; // embebido por defecto
        let mut p2 = DefinicionPaso::nuevo("b", 1);
        p2.ejecutor = Some("python".into());
        let mut p3 = DefinicionPaso::nuevo("c", 1);
        p3.ejecutor = Some("embebido".into());
        raiz.pasos_main = vec![p1, p2, p3];
        Programa {
            raiz,
            archivos: HashMap::new(),
            ejecutores: HashMap::from([
                (
                    "embebido".to_string(),
                    DefinicionEjecutor {
                        nombre: "embebido".into(),
                        tipo: TipoEjecutor::Embebido,
                    },
                ),
                (
                    "python".to_string(),
                    DefinicionEjecutor {
                        nombre: "python".into(),
                        tipo: TipoEjecutor::Grpc {
                            host: "127.0.0.1".into(),
                            puerto: 9101,
                        },
                    },
                ),
                (
                    "mi_paso".to_string(),
                    DefinicionEjecutor {
                        nombre: "mi_paso".into(),
                        tipo: TipoEjecutor::Wasm {
                            path: "./p.wasm".into(),
                        },
                    },
                ),
            ]),
        }
    }

    #[test]
    fn resolver_endpoint_embebido_por_defecto_y_por_nombre() {
        let programa = programa_ruteado();
        let p = DefinicionPaso::nuevo("a", 1);
        assert_eq!(Motor::resolver_endpoint(&p, &programa).unwrap(), EMBEDIDO);
        let mut p = DefinicionPaso::nuevo("b", 1);
        p.ejecutor = Some("embebido".into());
        assert_eq!(Motor::resolver_endpoint(&p, &programa).unwrap(), EMBEDIDO);
        let mut p = DefinicionPaso::nuevo("b", 1);
        p.ejecutor = Some("python".into());
        assert_eq!(Motor::resolver_endpoint(&p, &programa).unwrap(), "python");
    }

    #[test]
    fn resolver_endpoint_wasm_es_error_sin_host() {
        let programa = programa_ruteado();
        let mut p = DefinicionPaso::nuevo("x", 1);
        p.ejecutor = Some("mi_paso".into());
        let err = Motor::resolver_endpoint(&p, &programa).unwrap_err();
        assert!(matches!(err, Error::EjecutorWasmSinHost(ref n) if n == "mi_paso"));
        assert!(
            err.to_string().contains("anvil-host"),
            "apunta al host: {err}"
        );
    }

    #[test]
    fn resolver_endpoint_no_declarado_es_error() {
        let programa = programa_ruteado();
        let mut p = DefinicionPaso::nuevo("x", 1);
        p.ejecutor = Some("inventado".into());
        let err = Motor::resolver_endpoint(&p, &programa).unwrap_err();
        assert!(matches!(err, Error::EjecutorNoDeclarado(n) if n == "inventado"));
    }

    #[test]
    fn corre_un_paso_rutea_por_ejecutor() {
        let programa = programa_ruteado();
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorRuteado;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();
        assert_eq!(sec.pasos[0].mensaje, "embebido", "sin ejecutor → embebido");
        assert_eq!(sec.pasos[1].mensaje, "python", "ejecutor: python → python");
        assert_eq!(
            sec.pasos[2].mensaje, "embebido",
            "ejecutor: embebido explícito → embebido"
        );
    }

    /// El caso de DIAG-2 end-to-end: un veredicto compuesto falso **corta**
    /// Main y tiñe el agregado. Antes de ADR-0018 esto era un `statement` que
    /// asignaba a un local y la secuencia seguía en verde.
    #[test]
    fn pass_fail_falso_corta_main_y_tine_el_agregado() {
        let mut def = DefinicionSecuencia {
            nombre: "veredicto".into(),
            ..Default::default()
        };
        def.locals.insert("v".into(), ValorDefinicion::Numero(4.2));

        let mut verificar = DefinicionPaso::nuevo("verificar_dut", 1);
        verificar.tipo = TipoPaso::PassFail;
        verificar.condicion = Some(expr::parse_expresion("locals.v > 4.9").unwrap());

        let mut posterior = DefinicionPaso::nuevo("no_deberia_correr", 1);
        posterior.tipo = TipoPaso::Statement;
        posterior.statement = Some(expr::parse_sentencias("locals.v = 0.0").unwrap());

        def.pasos_main = vec![verificar, posterior];

        let programa = Programa {
            raiz: def,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, env) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        assert_eq!(sec.pasos.len(), 1, "Main corta en el pass_fail falso");
        assert_eq!(sec.pasos[0].estado, "fallo");
        assert_eq!(sec.estado(), "fallo", "el agregado ya no miente");
        assert_eq!(
            env.locals().get("v"),
            Some(&expr::Value::Numero(4.2)),
            "el paso posterior no llegó a correr"
        );
    }

    /// Y con la condición cumplida, la secuencia sigue.
    #[test]
    fn pass_fail_verdadero_deja_seguir_main() {
        let mut def = DefinicionSecuencia {
            nombre: "veredicto".into(),
            ..Default::default()
        };
        def.locals.insert("v".into(), ValorDefinicion::Numero(5.0));

        let mut verificar = DefinicionPaso::nuevo("verificar_dut", 1);
        verificar.tipo = TipoPaso::PassFail;
        verificar.condicion = Some(expr::parse_expresion("locals.v > 4.9").unwrap());

        let mut posterior = DefinicionPaso::nuevo("siguiente", 1);
        posterior.tipo = TipoPaso::Statement;
        posterior.statement = Some(expr::parse_sentencias("locals.v = 0.0").unwrap());

        def.pasos_main = vec![verificar, posterior];

        let programa = Programa {
            raiz: def,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        assert_eq!(sec.pasos.len(), 2);
        assert_eq!(sec.estado(), "paso");
    }

    // --- Fase del paso en el resultado (DIAG-3, #8) ---

    /// Un sink que anota `(nombre, fase)` de cada resultado que recibe: así
    /// el test comprueba que la fase está sellada **al emitir**, no sólo en
    /// el agregado final (un sink de streaming ve lo mismo).
    #[derive(Default)]
    struct SinkEspia {
        vistos: Vec<(String, Fase)>,
    }
    impl modelo::ResultSink for SinkEspia {
        fn on_resultado(&mut self, r: &ResultadoStep) {
            self.vistos.push((r.nombre.clone(), r.fase));
        }
    }

    /// Un `statement` inocuo con el nombre dado.
    fn stmt(nombre: &str) -> DefinicionPaso {
        let mut p = DefinicionPaso::nuevo(nombre, 1);
        p.tipo = TipoPaso::Statement;
        p.statement = Some(expr::parse_sentencias("locals.v = 1.0").unwrap());
        p
    }

    #[test]
    fn cada_paso_se_sella_con_la_fase_en_que_corrio() {
        let mut def = DefinicionSecuencia {
            nombre: "s".into(),
            ..Default::default()
        };
        def.locals.insert("v".into(), ValorDefinicion::Numero(0.0));
        def.pasos_setup = vec![stmt("conectar")];
        def.pasos_main = vec![stmt("medir"), {
            // Un `disable` también sale sellado: se emite sin invocar nada.
            let mut p = stmt("saltado");
            p.disable = true;
            p
        }];
        def.pasos_cleanup = vec![stmt("apagar")];

        let programa = Programa {
            raiz: def,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkEspia::default();
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        let fases: Vec<(&str, Fase)> = sec
            .pasos
            .iter()
            .map(|p| (p.nombre.as_str(), p.fase))
            .collect();
        assert_eq!(
            fases,
            vec![
                ("conectar", Fase::Setup),
                ("medir", Fase::Main),
                ("saltado", Fase::Main),
                ("apagar", Fase::Cleanup),
            ]
        );
        // El sink las vio ya selladas, en el mismo orden.
        assert_eq!(sink.vistos, fases_esperadas());
    }

    fn fases_esperadas() -> Vec<(String, Fase)> {
        vec![
            ("conectar".into(), Fase::Setup),
            ("medir".into(), Fase::Main),
            ("saltado".into(), Fase::Main),
            ("apagar".into(), Fase::Cleanup),
        ]
    }

    #[test]
    fn el_sequence_call_lleva_la_fase_del_padre_y_los_sub_pasos_la_suya() {
        // El call corre en el Setup del padre; dentro, la subsecuencia tiene
        // su propio Main. Cada nivel lleva la fase que le toca.
        let mut padre = DefinicionSecuencia {
            nombre: "padre".into(),
            ..Default::default()
        };
        padre
            .locals
            .insert("canal".into(), ValorDefinicion::Numero(1.0));
        padre
            .locals
            .insert("listo".into(), ValorDefinicion::Bool(false));
        padre
            .subsecuencias
            .insert("init".into(), inline_comprueba("init"));
        padre.pasos_setup = vec![call(
            "llamar",
            "init",
            vec![arg("canal", "canal"), arg("listo", "listo")],
        )];

        let programa = Programa {
            raiz: padre,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let entorno = EntornoMotor::desde_definicion(&programa.raiz);
        let mut inv = InvocadorMock;
        let mut sink = SinkNulo;
        let (sec, _) = ejecuta_secuencia_interna(
            &mut inv,
            &programa.raiz,
            entorno,
            &mut sink,
            &programa,
            0,
            true,
        )
        .unwrap();

        let call = &sec.pasos[0];
        assert_eq!(call.fase, Fase::Setup, "el call, con la fase del padre");
        let sub = call.sub_pasos.as_ref().unwrap();
        assert_eq!(sub[0].fase, Fase::Main, "el sub-paso, con la suya");
    }
}
