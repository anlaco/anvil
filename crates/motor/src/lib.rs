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

mod catalogo;
mod entorno;

use modelo::proto::{StepRequest, StepResult, Value as ProtoValue, CONTRACT, ROUTE_INVOKE};
use modelo::{
    Asignacion, DefinicionPaso, DefinicionSecuencia, EntradaPaso, Fase, Limite, Programa,
    ResultSink, ResultadoSecuencia, ResultadoStep, TipoEjecutor, TipoPaso,
};
use prost::Message;
use wasi_grpc::grpc::Cliente;
use wasi_grpc::net;

pub use catalogo::{
    comprueba_programa, endpoints_con_referencias, Catalogos, Descripcion, Hallazgo,
    Informe as InformeFirmas, SinComprobar,
};
pub use entorno::EntornoMotor;
use expr::{eval, eval_sentencias, Entorno, Expresion, Scope, Sentencia, Value};
use std::collections::HashMap;

/// Clave interna de la conexión al ejecutor embebido en `Motor.conexiones`.
/// No es declarable en el YAML: el cargador rechaza un ejecutor con este
/// nombre (ver `cargador::NOMBRE_EMBEDIDO_RESERVADO`).
pub const EMBEDIDO: &str = modelo::EJECUTOR_EMBEBIDO;

/// El motor: un cliente gRPC contra los ejecutores de pasos. M5-ext.1
/// (RF-36.3): despacha cada paso al endpoint del ejecutor que declara su
/// `DefinicionPaso.ejecutor`; sin declaración, va al embebido (`EMBEDIDO`).
/// La tabla `conexiones` se abre en `desde_programa` y cada `Grpc`
/// declarado tiene su `Cliente` propio.
pub struct Motor {
    /// Conexiones abiertas, keyed por nombre de ejecutor (o `EMBEDIDO`
    /// para el embebido). Un `TipoEjecutor::Wasm` **no** abre conexión: el
    /// motor nunca lo ejecuta (ADR-0014) — el host lo traduce a `grpc`
    /// (override `--executor`) antes de que llegue aquí; si llega sin
    /// traducir, `Error::EjecutorWasmSinHost`.
    conexiones: HashMap<String, Cliente>,
    /// The life each executor was on when the run started, for the endpoints
    /// that publish one (ADR-0022 §6). Filled by
    /// [`Motor::describe_ejecutores`], which already asks every endpoint once
    /// before the first step (ADR-0021 §3).
    ///
    /// An endpoint that is **absent** from this map publishes no lifetime, and
    /// that is a legitimate answer: references against it are then reported as
    /// unchecked for liveness and never asked about again. That is what keeps
    /// the check free for everyone who does not use references.
    vidas: HashMap<String, String>,
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
    /// expone como `grpc` (override `--executor`). El motor no ejecuta
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
        Ok(Motor {
            conexiones,
            vidas: HashMap::new(),
        })
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
    ///
    /// La regla de routing vive en el cargador ([`cargador::resolver_endpoint`])
    /// y aquí sólo se traduce a los errores del motor: el cargador la necesita
    /// también, para rechazar antes de arrancar una referencia que se le pasa
    /// a un paso de otro ejecutor (ADR-0022 §3), y dos copias de la regla es
    /// como el chequeo y el despacho dejan de estar de acuerdo.
    pub(crate) fn resolver_endpoint<'a>(
        def: &'a DefinicionPaso,
        programa: &'a Programa,
    ) -> Result<&'a str, Error> {
        match cargador::resolver_endpoint(def.ejecutor.as_deref(), &programa.ejecutores) {
            cargador::Endpoint::Embebido => Ok(EMBEDIDO),
            cargador::Endpoint::Grpc(n) => Ok(n),
            cargador::Endpoint::Wasm(n) => Err(Error::EjecutorWasmSinHost(n.to_string())),
            cargador::Endpoint::NoDeclarado(n) => Err(Error::EjecutorNoDeclarado(n.to_string())),
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
        parametros: &[(String, Value)],
    ) -> Result<ResultadoStep, Error> {
        let endpoint = Self::resolver_endpoint(def, programa)?.to_string();
        // Before anything crosses the wire (ADR-0022 §6: **before** invoking,
        // never on reading the result — a restart shows up on the next call,
        // and that call may be the very one carrying the dead handle).
        if let Some(r) = self.veredicto_de_las_referencias(def, &endpoint, parametros) {
            return Ok(r);
        }
        let endpoint = endpoint.as_str();
        let peticion = StepRequest {
            name: def.nombre.clone(),
            attempt: intento,
            inputs: parametros
                .iter()
                .filter_map(|(n, v)| ProtoValue::desde_value(n, v))
                .collect(),
            contract: CONTRACT,
        };
        let cliente = self
            .conexiones
            .get_mut(endpoint)
            .ok_or_else(|| Error::EjecutorNoConectado(endpoint.to_string()))?;
        let bytes = cliente.unaria(ROUTE_INVOKE, &peticion.encode_to_vec())?;
        let respuesta = StepResult::decode(&bytes[..])?;

        // El eco (ADR-0020 §4b). Un campo aditivo es «compatible» sólo en el
        // sentido de que el mensaje decodifica: un ejecutor de contrato 1
        // ignora `parametros`, **mide otra cosa y dice `paso`**. Ese es el
        // verde falso de ADR-0019 por una puerta nueva, y es lo único que
        // este número existe para impedir.
        //
        // Sólo se exige si el paso depende de lo nuevo. Si no declara
        // parámetros ni lee salidas, un ejecutor de contrato 1 sigue siendo
        // válido y no cambia nada — que es lo que mantiene vivo lo que ya
        // funciona.
        if let Some(r) = veredicto_del_eco(def, endpoint, respuesta.contract) {
            return Ok(r);
        }

        // Una salida sin tipo no se puede interpretar, y tragársela sería
        // inventarse un dato sobre la unidad (ADR-0019, Regla 2).
        let mut r = match respuesta.a_resultado() {
            Ok(r) => r,
            Err(e) => {
                return Ok(ResultadoStep::nuevo(
                    &def.nombre,
                    "error",
                    format!("el ejecutor '{}' devolvió {e}", nombre_visible(endpoint)),
                ))
            }
        };
        // Los parámetros no vuelven del cable: los sella el motor, que es
        // quien sabe qué envió. Es lo que hace que dos corridas con distinto
        // canal dejen de producir informes idénticos.
        r.parametros = parametros.to_vec();
        // El nombre del ejecutor lo estampa Anvil (ADR-0022 §4): el proceso de
        // enfrente no sabe cómo lo ha llamado la secuencia.
        if let Some(mal) = self.sella_referencias(&mut r, endpoint) {
            return Ok(mal);
        }
        Ok(r)
    }

    /// Stamps Anvil's half on every reference a step just minted, and refuses
    /// one that claims a life its executor is not on.
    ///
    /// The stamping is ADR-0022 §4: the executor cannot know what the sequence
    /// called it —the names live in `executors:`, on this side, which is also
    /// what routes— so Anvil writes the name and the executor writes the rest.
    ///
    /// The refusal covers the executor that is simply **wrong about itself**:
    /// it published one life in its catalog and minted a handle under another.
    /// Anvil cannot check the payload, but it can check this, and a handle
    /// nobody can resolve later is better refused now than spent at step 47.
    fn sella_referencias(&self, r: &mut ResultadoStep, endpoint: &str) -> Option<ResultadoStep> {
        let publicada = self.vidas.get(endpoint);
        for (nombre, valor) in r.salidas.iter_mut() {
            let Value::Reference(referencia) = valor else {
                continue;
            };
            if let Some(vida) = publicada {
                if !referencia.lifetime.is_empty() && referencia.lifetime != *vida {
                    return Some(ResultadoStep::nuevo(
                        &r.nombre,
                        "error",
                        format!(
                            "el ejecutor '{}' devolvió en '{nombre}' una referencia de la vida \
                             '{}', y su catálogo dice que está en la '{vida}'. Una referencia \
                             que no es de la vida de quien la acuña no la va a poder resolver \
                             nadie (ADR-0022 §6)",
                            nombre_visible(endpoint),
                            referencia.lifetime
                        ),
                    ));
                }
            }
            referencia.executor = endpoint.to_string();
        }
        None
    }

    /// What to do about the references this step is about to be handed, or
    /// `None` to go ahead and invoke.
    ///
    /// Two checks, in the order in which they cost:
    ///
    /// 1. **Whose handle is it.** A reference only means something inside the
    ///    executor that minted it; anywhere else it is a string that does not
    ///    match. The loader already refuses this by reading the file
    ///    (ADR-0022 §3), so getting here means the handle arrived by a route
    ///    the declaration did not describe — defence in depth, and free.
    /// 2. **Is that executor still the same one.** Not a type question at all:
    ///    the process opposite may have died and been born again, and no type
    ///    system answers that (ADR-0022 §6). The answer is asked for, once, and
    ///    only for a step that actually carries a handle to an endpoint that
    ///    publishes a lifetime — so a sequence that uses no references pays
    ///    nothing, and neither does an executor that publishes none.
    ///
    /// The verdict is a `ResultadoStep` in `error` and **never an abort**, and
    /// that is the decision ADR-0022 left open. A run that stops in its tracks
    /// does not run its `cleanup` (`ejecuta_secuencia_interna` returns before
    /// the closing loop), and the very moment this check fires is the moment
    /// Anvil most wants the step that closes the rack to run. Only an `error`
    /// gives it that chance. The step does not measure either way.
    ///
    /// Asking `Describe` again does not contradict ADR-0021 §3: **only the
    /// lifetime is read**, the signatures are ignored, so the catalog is still
    /// checked exactly once, at start-up, and the report stays reconstructible.
    fn veredicto_de_las_referencias(
        &mut self,
        def: &DefinicionPaso,
        endpoint: &str,
        parametros: &[(String, Value)],
    ) -> Option<ResultadoStep> {
        let referencias: Vec<(&str, &expr::Reference)> = parametros
            .iter()
            .filter_map(|(n, v)| v.reference().map(|r| (n.as_str(), r)))
            .collect();
        if referencias.is_empty() {
            return None;
        }

        for (nombre, r) in &referencias {
            if r.executor != endpoint {
                return Some(ResultadoStep::nuevo(
                    &def.nombre,
                    "error",
                    format!(
                        "el parámetro '{nombre}' lleva una referencia del ejecutor '{}' y este \
                         paso se despacha a '{}'. Una referencia sólo significa algo dentro del \
                         ejecutor que la acuñó (ADR-0022 §3)",
                        nombre_visible(&r.executor),
                        nombre_visible(endpoint)
                    ),
                ));
            }
        }

        // The endpoint publishes no lifetime: liveness is unchecked, said out
        // loud at start-up (`comprueba_firmas`) and not silently assumed. The
        // executor still rejects a foreign life on its own account — it is the
        // one that knows with certainty (ADR-0022 §6).
        self.vidas.get(endpoint)?;

        let ahora = match self.describe_uno(endpoint) {
            Descripcion::Describe(c) => Ok(c.lifetime),
            Descripcion::NoDescribe(motivo) => Err(motivo),
        };
        veredicto_de_vida(&def.nombre, endpoint, &referencias, ahora)
    }

    /// Corre un paso hasta que pase o se agoten los intentos.
    /// `reintentos` es el número **total** de intentos: 1 = un solo tiro.
    fn ejecuta_con_reintentos(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
        parametros: &[(String, Value)],
    ) -> Result<ResultadoStep, Error> {
        let max = def.reintentos.max(1);
        // Los mismos parámetros en todos los intentos: se evaluaron una vez,
        // antes de la primera llamada. Un reintento repite la medida, no
        // vuelve a resolver el entorno — si lo hiciera, dos intentos del
        // mismo paso podrían medir cosas distintas sin que se note.
        let mut resultado = self.ejecuta_paso(def, programa, 1, parametros)?;
        let mut intento = 1;
        while !resultado.paso() && intento < max {
            intento += 1;
            resultado = self.ejecuta_paso(def, programa, intento as i32, parametros)?;
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
    /// `"skipped"`), para que el lifecycle sea uniforme.
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
    /// `parametros` llega **ya evaluado** (ADR-0009/ADR-0020): quien resuelve
    /// las expresiones `${...}` es el motor, contra su entorno, antes de
    /// llegar aquí. El paso no ve `locals`; se le pasan valores.
    fn ejecuta_paso_grpc(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
        parametros: &[(String, Value)],
    ) -> Result<ResultadoStep, Error>;
}

impl InvocaPasos for Motor {
    fn ejecuta_paso_grpc(
        &mut self,
        def: &DefinicionPaso,
        programa: &Programa,
        parametros: &[(String, Value)],
    ) -> Result<ResultadoStep, Error> {
        self.ejecuta_con_reintentos(def, programa, parametros)
    }
}

/// Resuelve los parámetros de entrada de un paso `Grpc` (ADR-0020 §2) contra
/// el entorno del motor, **antes** de invocar.
///
/// `Ok` con la lista ya evaluada, en el orden determinista que fijó el
/// cargador. `Err` con el `ResultadoStep` en `error` si alguna expresión no se
/// pudo evaluar — y entonces el ejecutor no llega a llamarse. Va en `Box`
/// porque el caso de error es el raro y no debe pagarlo el `Ok` de cada paso.
///
/// Nunca hay valor por defecto: es la Regla 2 de ADR-0019 aplicada a la
/// entrada. Un paso que mide con un parámetro que el motor no supo resolver
/// devuelve un número que parece bueno, y eso es peor que no medir.
fn evalua_entradas(
    p: &DefinicionPaso,
    ent: &mut EntornoMotor,
) -> Result<Vec<(String, Value)>, Box<ResultadoStep>> {
    let Some(entradas) = &p.entradas else {
        return Ok(Vec::new());
    };
    // Sin resultado en curso: un parámetro se evalúa antes de que este paso
    // mida, así que `resultado.*` sería el del paso anterior. Igual que la
    // precondición (RF-33).
    ent.limpia_resultado();
    let mut fuera = Vec::with_capacity(entradas.len());
    for (nombre, entrada) in entradas {
        let v = match entrada {
            EntradaPaso::Literal(lit) => lit.a_value(),
            EntradaPaso::Expresion(e) => match eval(e, ent) {
                Ok(v) => v,
                Err(err) => {
                    return Err(Box::new(ResultadoStep::nuevo(
                        &p.nombre,
                        "error",
                        format!("el parámetro '{nombre}' no se pudo evaluar: {err}"),
                    )))
                }
            },
        };
        // Un `Nulo` no tiene representación en el cable. Mandarlo como
        // ausencia sería que el paso midiera sin ese parámetro y no se
        // enterase nadie.
        if v == Value::Nulo {
            return Err(Box::new(ResultadoStep::nuevo(
                &p.nombre,
                "error",
                format!(
                    "el parámetro '{nombre}' evaluó a 'nothing', y un paso no puede recibir \
                     un parámetro ausente: mediría sin él y nadie se enteraría"
                ),
            )));
        }
        fuera.push((nombre.clone(), v));
    }
    Ok(fuera)
}

/// Cómo se nombra un endpoint en un mensaje para el usuario. `EMBEDIDO` es
/// una clave interna que no se puede declarar en el YAML, así que enseñarla
/// tal cual sería enseñar un detalle de implementación.
pub(crate) fn nombre_visible(endpoint: &str) -> &str {
    modelo::nombre_visible_de_ejecutor(endpoint)
}

/// The liveness verdict, separated from the network so it can be tested
/// (ADR-0022 §6).
///
/// `ahora` is the life the executor claims **right now**, or why it could not
/// be asked. Each reference is compared against it and not against the life
/// recorded at start-up: what has to still be resolvable is the handle, and
/// comparing it directly is one fewer assumption than comparing two things
/// that are only equal because something else made them so.
///
/// A reference carrying **no** lifetime is left alone: that executor does not
/// stamp one, so there is nothing to compare and saying so is the honest
/// answer (ADR-0019, Rule 2).
fn veredicto_de_vida(
    paso: &str,
    endpoint: &str,
    referencias: &[(&str, &expr::Reference)],
    ahora: Result<String, String>,
) -> Option<ResultadoStep> {
    let ahora = match ahora {
        Ok(v) => v,
        Err(motivo) => {
            return Some(ResultadoStep::nuevo(
                paso,
                "error",
                format!(
                    "el ejecutor '{}' publicó su vida al arrancar y ahora no contesta \
                     ({motivo}). El paso lleva una referencia suya y no se invoca: medir \
                     contra un ejecutor que ya no se sabe si es el mismo sería medir contra \
                     otro banco (ADR-0022 §6)",
                    nombre_visible(endpoint)
                ),
            ))
        }
    };
    for (nombre, r) in referencias {
        if r.lifetime.is_empty() || r.lifetime == ahora {
            continue;
        }
        return Some(ResultadoStep::nuevo(
            paso,
            "error",
            format!(
                "el ejecutor '{}' se ha reiniciado a mitad de la corrida: el parámetro \
                 '{nombre}' lleva una referencia de la vida '{}' y el ejecutor dice estar \
                 ahora en la '{ahora}'. Esa referencia ya no apunta a nada, así que el paso \
                 no se invoca y no mide (ADR-0022 §6)",
                nombre_visible(endpoint),
                r.lifetime
            ),
        ));
    }
    None
}

/// La comprobación del eco (ADR-0020 §4b), aislada de la red para poder
/// probarla.
///
/// `Some(error)` si el paso depende del contrato 2 y el ejecutor respondió
/// uno menor; `None` si la llamada es legítima.
///
/// **Por qué existe este número.** Un campo aditivo es «compatible» sólo en
/// el sentido de que el mensaje decodifica. Un ejecutor de contrato 1 ignora
/// `parametros` —proto3 se lo permite—, mide con lo que tuviera dentro y
/// devuelve `paso`. Eso es un verde falso sobre una unidad que no se ha
/// probado como dice la secuencia, y no hay ninguna otra señal que lo delate.
///
/// Y el recíproco es lo que mantiene vivo lo que ya funciona: un paso que no
/// pide nada nuevo corre contra un ejecutor de contrato 1 igual que siempre.
pub(crate) fn veredicto_del_eco(
    def: &DefinicionPaso,
    endpoint: &str,
    eco: i32,
) -> Option<ResultadoStep> {
    if !necesita_contrato_2(def) || eco >= CONTRACT {
        return None;
    }
    // Un ejecutor de contrato 1 no conoce el tag del eco y devuelve `0` por
    // el default de proto3. Enseñar ese `0` tal cual sería enseñar un detalle
    // de protobuf: para quien lee el error, el ejecutor habla el contrato 1.
    let cual = if eco == 0 {
        "1 (no declara versión, que es como se reconoce al contrato 1)".to_string()
    } else {
        eco.to_string()
    };
    Some(ResultadoStep::nuevo(
        &def.nombre,
        "error",
        format!(
            "el ejecutor '{}' entiende el contrato {cual} y este paso necesita el {CONTRACT}: \
             sus 'parametros' se habrían perdido sin aviso y habría medido otra cosa. \
             Recompila o actualiza ese ejecutor.",
            nombre_visible(endpoint),
        ),
    ))
}

/// Si este paso depende del contrato 2 (ADR-0020 §4b): declara parámetros de
/// entrada, o su `asigna` lee alguna `result.outputs.<nombre>`.
///
/// El recíproco es lo que mantiene vivo lo que ya funciona: un paso que no
/// pide nada de lo nuevo corre contra un ejecutor de contrato 1 igual que
/// antes.
fn necesita_contrato_2(def: &DefinicionPaso) -> bool {
    if def.entradas.is_some() {
        return true;
    }
    def.asigna
        .as_deref()
        .is_some_and(|asigs| asigs.iter().any(|a| lee_salidas(&a.expr)))
}

/// `true` si el AST lee en algún sitio una `result.outputs.<nombre>`.
/// Recorre el árbol entero: la lectura puede estar dentro de una operación
/// (`result.outputs.t * 2`), no sólo suelta.
fn lee_salidas(e: &Expresion) -> bool {
    match e {
        Expresion::Var { scope, campo } => {
            *scope == Scope::Resultado
                && campo
                    .strip_prefix(expr::CAMPO_SALIDAS)
                    .is_some_and(|r| r.starts_with('.'))
        }
        Expresion::BinOp { izq, der, .. } => lee_salidas(izq) || lee_salidas(der),
        Expresion::UnOp { operando, .. } => lee_salidas(operando),
        Expresion::Lit(_) => false,
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
        let fallo = !r.paso() && r.estado != "skipped";
        secuencia.registra(r.clone());
        if fallo {
            setup_ok = false;
        }
        if p.pause_on_fail && fallo {
            break;
        }
    }

    // --- Main: solo si el Setup fue bien; corta en el primer fallo. ---
    // Se anota por el camino si algún `pass_fail` llegó a evaluarse: es lo que
    // decide el `inconcluso` de abajo, y aquí se sabe sin tener que emparejar
    // resultados con definiciones a posteriori.
    let mut veredicto_evaluado = false;
    if setup_ok {
        for p in &def.pasos_main {
            let r = corre_un_paso(inv, p, &mut entorno, sink, &ctx(Fase::Main))?;
            let fallo = !r.paso() && r.estado != "skipped";
            if p.tipo == TipoPaso::PassFail && r.estado != "skipped" {
                veredicto_evaluado = true;
            }
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

    // El veredicto declarado y no evaluado (ADR-0019, Regla 1, issue #31): la
    // secuencia declara al menos un `pass_fail` en Main y ninguno se evaluó —
    // porque se saltó, o porque el Main ni llegó a él. Se sella **antes** de
    // `on_fin_secuencia` para que consola, JSON y CSV vean ya el agregado
    // correcto; y sólo eleva la severidad, así que un `fallo` de Setup o un
    // `error` que también estén presentes siguen mandando.
    //
    // Una secuencia cuyo criterio son los `limite` de sus pasos no declara
    // ningún `pass_fail` y no cambia de comportamiento.
    //
    // Un `pass_fail` con `disable: true` cuenta como declarado: la unidad
    // tampoco se ha medido. Eximirlo convertiría el `disable` en una puerta
    // trasera al verde falso, que es justo lo que la Regla 1 cierra; si algún
    // día un salto intencionado debe tratarse distinto, eso es criterio del
    // usuario y vive en `--strict` (#13, #23), no aquí.
    secuencia.veredicto_sin_evaluar =
        !veredicto_evaluado && def.pasos_main.iter().any(|p| p.tipo == TipoPaso::PassFail);

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
        let r = sella(ResultadoStep::nuevo(&p.nombre, "skipped", "disable"));
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
                let r = sella(*r);
                sink.on_resultado(&r);
                sink.on_fin_paso(p);
                return Ok(r);
            }
        }
    }

    // (c) según tipo de paso (RF-27). Lo que viene de un ejecutor se normaliza
    // aquí mismo (ADR-0019, Regla 2): es el único punto por el que pasa todo
    // resultado gRPC, mock de test incluido. Los otros tres tipos los produce
    // el motor, que no se equivoca de vocabulario — y el `sequence_call`
    // devuelve además `"inconclusive"`, que es agregado legítimo y no estado de
    // ejecutor.
    let mut r = match p.tipo {
        TipoPaso::Statement => ejecuta_statement_puro(p.statement.as_deref(), &p.nombre, ent),
        TipoPaso::PassFail => evalua_pass_fail(p.condicion.as_ref(), &p.nombre, ent),
        // ADR-0020: los parámetros se evalúan **aquí**, donde está el entorno,
        // y antes de invocar. Una expresión que falla convierte el paso en
        // `error` y **no se llama al ejecutor**: medir con un parámetro
        // inventado da un número que parece bueno y no lo es.
        TipoPaso::Grpc => match evalua_entradas(p, ent) {
            Ok(parametros) => {
                let r = inv.ejecuta_paso_grpc(p, ctx.programa, &parametros)?;
                normaliza_estado_de_ejecutor(r)
            }
            Err(r) => *r,
        },
        TipoPaso::SequenceCall => ejecuta_sequence_call(inv, p, ent, sink, ctx)?,
    };

    // (d) asigna (RF-31): tras un paso Grpc o SequenceCall, vuelca campos
    // de `resultado` a Locals. Un statement asigna dentro de su sentencia; un
    // pass_fail no produce `resultado.*` que volcar (el cargador rechaza
    // `asigna` en un pass_fail y en un statement, así que aquí no hay nada que
    // ignorar).
    //
    // **No se asigna si el paso dio `error`** (ADR-0019, Regla 2): no hay
    // resultado del que volcar nada, y lo que hacía antes era escribir el
    // `nothing` de un `resultado.*` vacío encima de una variable con valor
    // bueno. Que la variable la lea después un `cleanup` para decidir si apaga
    // una fuente es todo el argumento: el destino no se toca.
    if matches!(p.tipo, TipoPaso::Grpc | TipoPaso::SequenceCall) && r.estado != "error" {
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
/// a registrar, ya sea `"skipped"` o `"error"`).
enum VeredictoPre {
    Continua,
    // `Box` porque `ResultadoStep` creció con `parametros` y `salidas`
    // (ADR-0020) y el enum entero pasaría a ocupar lo que ocupa el caso
    // raro, en todas las llamadas del caso normal.
    Salta(Box<ResultadoStep>),
}

/// Evalúa la precondición contra el entorno. Es **pura** (sin gRPC): la
/// prueba el motor sin levantar red. La precondición **no** ve el resultado
/// del paso (aún no corre), así que se limpia antes.
fn evalua_precondicion(pre: &Expresion, ent: &mut EntornoMotor, nombre: &str) -> VeredictoPre {
    ent.limpia_resultado();
    match eval(pre, ent) {
        Ok(Value::Bool(true)) => VeredictoPre::Continua,
        Ok(Value::Bool(false)) => VeredictoPre::Salta(Box::new(ResultadoStep::nuevo(
            nombre,
            "skipped",
            "precondición falsa",
        ))),
        Ok(v) => VeredictoPre::Salta(Box::new(ResultadoStep::nuevo(
            nombre,
            "error",
            format!("precondición: se esperaba bool, no {}", v.tipo()),
        ))),
        Err(e) => VeredictoPre::Salta(Box::new(ResultadoStep::nuevo(
            nombre,
            "error",
            format!("precondición: {e}"),
        ))),
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
        Ok(()) => ResultadoStep::nuevo(nombre, "pass", "statement ok"),
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
        Ok(Value::Bool(true)) => ResultadoStep::nuevo(nombre, "pass", "condición cumplida"),
        Ok(Value::Bool(false)) => ResultadoStep::nuevo(nombre, "fail", "condición no cumplida"),
        Ok(v) => ResultadoStep::nuevo(
            nombre,
            "error",
            format!("condición: se esperaba bool, no {}", v.tipo()),
        ),
        Err(e) => ResultadoStep::nuevo(nombre, "error", format!("condición: {e}")),
    }
}

/// Convierte en `"error"` un `ResultadoStep` cuyo `estado` no es ninguno de
/// los cuatro que un ejecutor puede devolver (ADR-0019, Regla 2, issue #28).
///
/// `fallo` es del DUT; `error` es de Anvil. Que un ejecutor escriba `"Paso"` o
/// `"PASS"` no dice nada sobre la unidad: dice que el ejecutor no habla el
/// contrato. Antes esto acababa en `fallo` mudo; al introducir la escala de
/// severidad pasó a `paso` mudo, que es peor. Aquí deja de ser mudo.
///
/// El mensaje nombra el valor recibido y enumera los válidos porque quien tiene
/// que arreglarlo es el autor de un ejecutor de terceros, que no va a leer el
/// código de Anvil para averiguar cuáles son.
fn normaliza_estado_de_ejecutor(mut r: ResultadoStep) -> ResultadoStep {
    if modelo::ESTADOS_DE_EJECUTOR.contains(&r.estado.as_str()) {
        return r;
    }
    r.mensaje = format!(
        "el ejecutor devolvió el estado '{}', que no es ninguno de {}: \
         Anvil no juzga la unidad con un estado que no entiende \
         (el paso decía: '{}')",
        r.estado,
        modelo::ESTADOS_DE_EJECUTOR
            .map(|e| format!("'{e}'"))
            .join(", "),
        r.mensaje
    );
    r.estado = "error".into();
    r
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
    if r.estado == "pass" && lim.evalua(valor) == "fail" {
        r.estado = "fail".into();
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

    /// Un paso que midió `valor` y devuelve `estado` ("pass"/"fail"/"error"),
    /// sin conocer el umbral: es lo que produce un paso de *limit test* en M3.
    fn paso_medido(valor: f64, estado: &str) -> ResultadoStep {
        ResultadoStep::medido_valor("medir_voltaje", estado, "medido", valor)
    }

    #[test]
    fn rango_dentro_deja_paso_y_rellena_campos() {
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(5.0, "pass"));
        assert_eq!(r.estado, "pass");
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
        let r = aplicar_limite(&def, paso_medido(4.2, "pass"));
        assert_eq!(r.estado, "fail");
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
        let r = aplicar_limite(&def, paso_medido(999.0, "pass"));
        assert_eq!(r.estado, "fail");
        assert_eq!(r.operador, Some(Operador::Ge));
        assert_eq!(r.valor_esperado, Some(1000.0));
        assert_eq!(r.mensaje, "999 >= 1000 no cumplido");
    }

    #[test]
    fn el_paso_que_ya_fallo_no_se_mejora_solo_se_rellena_el_limite() {
        // El paso sabe algo que el límite no: su fallo se respeta.
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, paso_medido(5.0, "fail"));
        assert_eq!(r.estado, "fail", "el paso ya falló: el límite no lo mejora");
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
        let r = aplicar_limite(&def, paso_medido(4.2, "pass"));
        assert_eq!(r.estado, "pass");
        assert_eq!(r.limite_min, None);
    }

    #[test]
    fn paso_con_limite_pero_sin_medida_no_se_evalua() {
        // Un pass/fail con un límite declarado (mal uso) no debe pánico: sin
        // valor_medido el límite no aplica, todo se queda igual.
        let def = DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 4.5, max: 5.5 });
        let r = aplicar_limite(&def, ResultadoStep::nuevo("m", "pass", "sin medida"));
        assert_eq!(r.estado, "pass");
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
        assert_eq!(r.estado, "skipped");
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
        assert_eq!(r.estado, "pass");
        assert_eq!(r.nombre, "verificar_dut");
    }

    /// Lo que DIAG-2 no permitía expresar: un veredicto compuesto que **falla**.
    #[test]
    fn pass_fail_condicion_falsa_falla() {
        let mut env = entorno_con_locals(&[("v", ValorDefinicion::Numero(4.2))]);
        let cond = expr::parse_expresion("locals.v > 4.9").unwrap();
        let r = evalua_pass_fail(Some(&cond), "verificar_dut", &mut env);
        assert_eq!(r.estado, "fail", "un veredicto falso falla el paso");
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
        assert_eq!(r.estado, "pass");
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
        let res = ResultadoStep::medido_valor("m", "pass", "ok", 4.2);
        let asignaciones = vec![modelo::Asignacion {
            var: "voltaje".into(),
            expr: expr::parse_expresion("result.measured_value").unwrap(),
        }];
        let r = aplica_asigna(&asignaciones, res, &mut env);
        assert_eq!(r.estado, "pass", "el paso ya pasó; la asigna no falla");
        assert_eq!(env.locals().get("voltaje"), Some(&expr::Value::Numero(4.2)));
    }

    #[test]
    fn asigna_que_falla_convierte_el_paso_en_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(0.0))]);
        let res = ResultadoStep::medido_valor("m", "pass", "ok", 4.2);
        // Leer un local no declarado es error de evaluación (lectura estricta
        // de `locals`), así que la asigna falla.
        let asignaciones = vec![modelo::Asignacion {
            var: "x".into(),
            expr: expr::parse_expresion("locals.no_declarado + 1").unwrap(),
        }];
        let r = aplica_asigna(&asignaciones, res, &mut env);
        assert_eq!(
            r.estado, "error",
            "una asigna que falla es un fallo de definición"
        );
        assert!(r.mensaje.contains("asigna"));
    }

    /// ADR-0019, Regla 2 (issue #27): un campo inexistente de `resultado` toma
    /// el camino que ya existía —una asigna que falla convierte el paso en
    /// `error`— en vez de volcar un `nothing` mudo. El cargador lo caza antes
    /// de ejecutar; esto prueba la red de debajo.
    #[test]
    fn asigna_desde_un_campo_inexistente_de_resultado_es_error() {
        let mut env = entorno_con_locals(&[("x", ValorDefinicion::Numero(0.0))]);
        let res = ResultadoStep::medido_valor("m", "pass", "ok", 4.2);
        let asignaciones = vec![modelo::Asignacion {
            var: "x".into(),
            expr: expr::parse_expresion("result.measured_valu").unwrap(),
        }];
        let r = aplica_asigna(&asignaciones, res, &mut env);
        assert_eq!(r.estado, "error", "un typo no puede pasar por dato ausente");
        assert!(
            r.mensaje.contains("measured_valu") && r.mensaje.contains("'measured_value'"),
            "nombra el campo escrito y los válidos: {}",
            r.mensaje
        );
        assert_eq!(
            env.locals().get("x"),
            Some(&expr::Value::Numero(0.0)),
            "y no toca el destino"
        );
    }

    // --- ADR-0019, Regla 2: `fallo` es del DUT, `error` es de Anvil ---

    /// Issue #28. El mensaje tiene que servirle al autor de un ejecutor de
    /// terceros sin que lea el código de Anvil: nombra el valor recibido y
    /// enumera los cuatro válidos.
    #[test]
    fn un_estado_no_reconocido_se_convierte_en_error() {
        let r = normaliza_estado_de_ejecutor(ResultadoStep::nuevo(
            "verificar_led",
            "Paso",
            "led encendido",
        ));
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("'Paso'"), "el valor: {}", r.mensaje);
        for e in modelo::ESTADOS_DE_EJECUTOR {
            assert!(
                r.mensaje.contains(&format!("'{e}'")),
                "enumera '{e}': {}",
                r.mensaje
            );
        }
        assert!(
            r.mensaje.contains("led encendido"),
            "conserva lo que el paso decía: {}",
            r.mensaje
        );
    }

    /// Los cuatro válidos pasan intactos, mensaje incluido. `inconcluso` no
    /// está entre ellos a propósito: lo produce el motor al agregar, y un
    /// ejecutor que lo devuelva cae bajo la misma regla (ADR-0019, «Recortes»).
    #[test]
    fn los_estados_validos_pasan_intactos_y_inconcluso_no_es_uno() {
        for e in modelo::ESTADOS_DE_EJECUTOR {
            let r = normaliza_estado_de_ejecutor(ResultadoStep::nuevo("p", e, "tal cual"));
            assert_eq!(r.estado, e);
            assert_eq!(r.mensaje, "tal cual");
        }
        let r = normaliza_estado_de_ejecutor(ResultadoStep::nuevo("p", "inconclusive", "m"));
        assert_eq!(
            r.estado, "error",
            "un ejecutor no puede declararse a sí mismo no concluyente"
        );
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
            _parametros: &[(String, Value)],
        ) -> Result<ResultadoStep, Error> {
            panic!("InvocadorMock no espera pasos grpc en estos tests");
        }
    }

    /// Un sink que no hace nada (sólo nos interesa el `ResultadoSecuencia`).
    struct SinkNulo;
    impl modelo::ResultSink for SinkNulo {}

    /// Un invocador que devuelve siempre el mismo estado y mensaje: sirve para
    /// probar qué hace el motor con lo que le entrega un ejecutor, sin red.
    struct InvocadorFijo {
        estado: &'static str,
        mensaje: &'static str,
    }
    impl InvocaPasos for InvocadorFijo {
        fn ejecuta_paso_grpc(
            &mut self,
            def: &DefinicionPaso,
            _programa: &Programa,
            _parametros: &[(String, Value)],
        ) -> Result<ResultadoStep, Error> {
            Ok(ResultadoStep::nuevo(&def.nombre, self.estado, self.mensaje))
        }
    }

    /// Corre `def` con el invocador dado y devuelve secuencia y entorno final.
    fn corre_con(
        inv: &mut impl InvocaPasos,
        def: &DefinicionSecuencia,
    ) -> (ResultadoSecuencia, EntornoMotor) {
        let programa = Programa {
            raiz: def.clone(),
            ..Default::default()
        };
        let entorno = EntornoMotor::desde_definicion(def);
        ejecuta_secuencia_interna(inv, def, entorno, &mut SinkNulo, &programa, 0, true).unwrap()
    }

    /// Un paso `Grpc` con `asigna` sobre `valor_medido`.
    fn paso_grpc_que_asigna(nombre: &str, destino: &str) -> DefinicionPaso {
        let mut p = DefinicionPaso::nuevo(nombre, 1);
        p.asigna = Some(vec![modelo::Asignacion {
            var: destino.into(),
            expr: expr::parse_expresion("result.measured_value").unwrap(),
        }]);
        p
    }

    /// ADR-0019, Regla 2 (issue #28), extremo a extremo: el ejecutor devuelve
    /// `"Paso"` con mayúscula, el paso queda en `error` y la secuencia **no**
    /// sale verde. Antes de la Regla 1 esto era un `fallo` mudo; después, un
    /// `paso` mudo, que es peor.
    #[test]
    fn un_estado_no_reconocido_no_deja_verde_la_secuencia() {
        let def = DefinicionSecuencia {
            nombre: "r28".into(),
            pasos_main: vec![DefinicionPaso::nuevo("verificar_led", 1)],
            ..Default::default()
        };
        let mut inv = InvocadorFijo {
            estado: "Paso",
            mensaje: "led encendido",
        };
        let (s, _) = corre_con(&mut inv, &def);
        assert_eq!(s.estado(), "error");
        assert_eq!(s.pasos[0].estado, "error");
        assert!(s.pasos[0].mensaje.contains("'Paso'"));
    }

    /// ADR-0019, Regla 2 (issue #27, caso 2): si el paso dio `error` no hay
    /// resultado del que volcar nada, así que `asigna` no escribe. Antes
    /// borraba con un `nothing` la variable que el `cleanup` iba a usar para
    /// decidir si apagaba una fuente.
    #[test]
    fn asigna_no_toca_el_destino_si_el_paso_dio_error() {
        let mut def = DefinicionSecuencia {
            nombre: "r27d".into(),
            pasos_main: vec![paso_grpc_que_asigna("paso_inexistente", "valor")],
            ..Default::default()
        };
        def.locals
            .insert("valor".into(), ValorDefinicion::Numero(99.0));

        let mut inv = InvocadorFijo {
            estado: "error",
            mensaje: "paso no reconocido",
        };
        let (s, env) = corre_con(&mut inv, &def);
        assert_eq!(s.estado(), "error");
        assert_eq!(
            env.locals().get("valor"),
            Some(&expr::Value::Numero(99.0)),
            "el valor bueno sigue ahí"
        );
    }

    /// La otra mitad de la regla: si el paso **no** dio error, `asigna` sigue
    /// funcionando exactamente como antes. Lo que se acota es el caso sin
    /// resultado, no la funcionalidad.
    #[test]
    fn asigna_sigue_escribiendo_cuando_el_paso_no_da_error() {
        let mut def = DefinicionSecuencia {
            nombre: "ok".into(),
            pasos_main: vec![paso_grpc_que_asigna("medir", "valor")],
            ..Default::default()
        };
        def.locals
            .insert("valor".into(), ValorDefinicion::Numero(0.0));

        struct InvocadorQueMide;
        impl InvocaPasos for InvocadorQueMide {
            fn ejecuta_paso_grpc(
                &mut self,
                def: &DefinicionPaso,
                _programa: &Programa,
                _parametros: &[(String, Value)],
            ) -> Result<ResultadoStep, Error> {
                Ok(ResultadoStep::medido_valor(
                    &def.nombre,
                    "pass",
                    "medido: 4.2 V",
                    4.2,
                ))
            }
        }
        let (s, env) = corre_con(&mut InvocadorQueMide, &def);
        assert_eq!(s.estado(), "pass");
        assert_eq!(env.locals().get("valor"), Some(&expr::Value::Numero(4.2)));
    }

    /// Un `fallo` del DUT no es un error de Anvil: su `asigna` sí corre, porque
    /// hay resultado — la medida que falló el límite es justo lo que un
    /// `cleanup` o un informe quieren leer.
    #[test]
    fn asigna_corre_cuando_el_paso_falla() {
        let mut def = DefinicionSecuencia {
            nombre: "f".into(),
            pasos_main: vec![paso_grpc_que_asigna("medir", "valor")],
            ..Default::default()
        };
        def.locals
            .insert("valor".into(), ValorDefinicion::Numero(0.0));

        struct InvocadorQueFalla;
        impl InvocaPasos for InvocadorQueFalla {
            fn ejecuta_paso_grpc(
                &mut self,
                def: &DefinicionPaso,
                _programa: &Programa,
                _parametros: &[(String, Value)],
            ) -> Result<ResultadoStep, Error> {
                Ok(ResultadoStep::medido_valor(
                    &def.nombre,
                    "fail",
                    "fuera de rango",
                    6.1,
                ))
            }
        }
        let (s, env) = corre_con(&mut InvocadorQueFalla, &def);
        assert_eq!(s.estado(), "fail");
        assert_eq!(env.locals().get("valor"), Some(&expr::Value::Numero(6.1)));
    }

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
        assert_eq!(r.estado, "pass");
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

        assert_eq!(sec.pasos[0].estado, "pass");
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
        // statement cuyo `estado` es "pass"; el agregado de la sub es "pass".
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

        assert_eq!(sec.pasos[0].estado, "pass");
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
            _parametros: &[(String, Value)],
        ) -> Result<ResultadoStep, Error> {
            let mensaje = match def.ejecutor.as_deref() {
                None => "embebido",
                Some(n) => n,
            };
            Ok(ResultadoStep::nuevo(&def.nombre, "pass", mensaje))
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
        assert_eq!(sec.pasos[0].estado, "fail");
        assert_eq!(sec.estado(), "fail", "el agregado ya no miente");
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
        assert_eq!(sec.estado(), "pass");
        assert!(
            !sec.veredicto_sin_evaluar,
            "el veredicto se evaluó: nada que declarar"
        );
    }

    /// El issue #31 end-to-end, con el motor de verdad: el único `pass_fail`
    /// de Main se salta por precondición falsa. No hay ningún paso en rojo, y
    /// aun así la unidad no se ha medido.
    #[test]
    fn un_pass_fail_saltado_deja_la_secuencia_inconclusa() {
        let mut def = DefinicionSecuencia {
            nombre: "b31".into(),
            ..Default::default()
        };
        def.locals
            .insert("flag".into(), ValorDefinicion::Bool(false));

        let mut init = DefinicionPaso::nuevo("init", 1);
        init.tipo = TipoPaso::Statement;
        init.statement = Some(expr::parse_sentencias("locals.flag = false").unwrap());

        let mut verdict = DefinicionPaso::nuevo("verdict", 1);
        verdict.tipo = TipoPaso::PassFail;
        verdict.precondicion = Some(expr::parse_expresion("locals.flag").unwrap());
        verdict.condicion = Some(expr::parse_expresion("locals.flag == true").unwrap());

        def.pasos_main = vec![init, verdict];

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

        assert_eq!(
            sec.pasos[1].estado, "skipped",
            "el paso se sigue reportando como lo que fue"
        );
        assert!(sec.veredicto_sin_evaluar);
        assert_eq!(
            sec.estado(),
            "inconclusive",
            "el veredicto no se evaluó: la secuencia no puede afirmar `paso`"
        );
    }

    /// Un `pass_fail` deshabilitado tampoco mide nada. Eximir a `disable`
    /// convertiría el flag en una puerta trasera al verde falso; si un salto
    /// intencionado debe tratarse distinto, eso es `--strict` (#13, #23).
    #[test]
    fn un_pass_fail_con_disable_tambien_deja_inconcluso() {
        let mut def = DefinicionSecuencia {
            nombre: "d".into(),
            ..Default::default()
        };
        let mut verdict = DefinicionPaso::nuevo("verdict", 1);
        verdict.tipo = TipoPaso::PassFail;
        verdict.disable = true;
        verdict.condicion = Some(expr::parse_expresion("true").unwrap());
        def.pasos_main = vec![verdict];

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

        assert_eq!(sec.estado(), "inconclusive");
    }

    /// La otra mitad de la Regla 1, y la que evita una regresión masiva: una
    /// secuencia cuyo criterio son los `limite` de sus pasos **no cambia de
    /// comportamiento**, aunque tenga pasos saltados. No declara ningún
    /// `pass_fail`, así que ahí el veredicto sí se evaluó, paso a paso.
    #[test]
    fn sin_pass_fail_declarado_un_salto_sigue_siendo_neutral() {
        let mut def = DefinicionSecuencia {
            nombre: "limites".into(),
            ..Default::default()
        };
        def.locals
            .insert("flag".into(), ValorDefinicion::Bool(false));

        let mut medir = DefinicionPaso::nuevo("medir", 1);
        medir.tipo = TipoPaso::Statement;
        medir.statement = Some(expr::parse_sentencias("locals.flag = false").unwrap());

        let mut opcional = DefinicionPaso::nuevo("verificar_led", 1);
        opcional.tipo = TipoPaso::Statement;
        opcional.statement = Some(expr::parse_sentencias("locals.flag = true").unwrap());
        opcional.precondicion = Some(expr::parse_expresion("locals.flag").unwrap());

        def.pasos_main = vec![medir, opcional];

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

        assert_eq!(sec.pasos[1].estado, "skipped");
        assert!(!sec.veredicto_sin_evaluar);
        assert_eq!(sec.estado(), "pass", "el criterio eran los límites");
    }

    /// Un `fallo` presente manda sobre el `inconcluso`: si el Setup se rompe,
    /// el Main ni corre —así que el veredicto tampoco se evalúa— y aun así lo
    /// que hay que reportar es el fallo, no la ausencia de medida.
    #[test]
    fn un_fallo_de_setup_manda_sobre_el_veredicto_sin_evaluar() {
        let mut def = DefinicionSecuencia {
            nombre: "s".into(),
            ..Default::default()
        };
        let mut setup = DefinicionPaso::nuevo("comprobar_banco", 1);
        setup.tipo = TipoPaso::PassFail;
        setup.condicion = Some(expr::parse_expresion("false").unwrap());
        def.pasos_setup = vec![setup];

        let mut verdict = DefinicionPaso::nuevo("verdict", 1);
        verdict.tipo = TipoPaso::PassFail;
        verdict.condicion = Some(expr::parse_expresion("true").unwrap());
        def.pasos_main = vec![verdict];

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

        assert!(
            sec.veredicto_sin_evaluar,
            "el Main no corrió: el veredicto no se evaluó"
        );
        assert_eq!(sec.estado(), "fail", "pero el fallo del Setup manda");
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
            let mut p = stmt("skipped");
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
                ("skipped", Fase::Main),
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
            ("skipped".into(), Fase::Main),
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

    // ---------------------------------------------------------------------
    // ADR-0022: lo que el motor hace con una referencia sin tocar la red.
    // ---------------------------------------------------------------------

    /// A `Motor` with no connections: enough for everything that decides
    /// **before** the wire, which is where these checks live.
    fn motor_con_vidas(vidas: &[(&str, &str)]) -> Motor {
        Motor {
            conexiones: HashMap::new(),
            vidas: vidas
                .iter()
                .map(|(e, v)| ((*e).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    fn referencia(ejecutor: &str, vida: &str, payload: &str) -> Value {
        Value::Reference(expr::Reference {
            executor: ejecutor.into(),
            lifetime: vida.into(),
            payload: payload.into(),
        })
    }

    /// El nombre del ejecutor lo pone Anvil, no el ejecutor (ADR-0022 §4): el
    /// proceso de enfrente no sabe cómo lo ha llamado la secuencia.
    ///
    /// Visto fallar quitando la línea que estampa `referencia.executor`: la
    /// referencia sale con el nombre vacío que puso el ejecutor, y el chequeo
    /// cruzado de la llamada siguiente la rechazaría por «del ejecutor ''».
    #[test]
    fn anvil_estampa_el_nombre_del_ejecutor_en_la_referencia() {
        let motor = motor_con_vidas(&[("banco", "v1")]);
        let mut r = ResultadoStep::nuevo("abrir", "pass", "ok");
        r.salidas = vec![("rack".into(), referencia("", "v1", "s1"))];
        assert!(motor.sella_referencias(&mut r, "banco").is_none());
        assert_eq!(
            r.salidas[0].1.reference().unwrap().executor,
            "banco",
            "el nombre lo pone el motor"
        );
    }

    /// Un ejecutor que publica una vida en su catálogo y acuña con otra está
    /// equivocado sobre sí mismo, y la referencia no la va a resolver nadie.
    #[test]
    fn una_referencia_de_una_vida_que_no_es_la_del_ejecutor_es_error() {
        let motor = motor_con_vidas(&[("banco", "v1")]);
        let mut r = ResultadoStep::nuevo("abrir", "pass", "ok");
        r.salidas = vec![("rack".into(), referencia("", "v9", "s1"))];
        let mal = motor
            .sella_referencias(&mut r, "banco")
            .expect("una vida ajena no pasa");
        assert_eq!(mal.estado, "error");
        assert!(
            mal.mensaje.contains("v9") && mal.mensaje.contains("v1"),
            "{}",
            mal.mensaje
        );
    }

    /// Defensa en profundidad del chequeo cruzado: el cargador ya lo rechaza
    /// leyendo el fichero, así que llegar aquí significa que la referencia
    /// entró por una ruta que la declaración no describía.
    #[test]
    fn una_referencia_de_otro_ejecutor_no_llega_a_invocar() {
        let mut motor = motor_con_vidas(&[("banco", "v1")]);
        let def = DefinicionPaso::nuevo("medir", 1);
        let params = vec![("rack".to_string(), referencia("banco", "v1", "s1"))];
        let r = motor
            .veredicto_de_las_referencias(&def, "otro", &params)
            .expect("no se invoca");
        assert_eq!(r.estado, "error");
        assert!(
            r.mensaje.contains("banco") && r.mensaje.contains("otro"),
            "{}",
            r.mensaje
        );
    }

    /// Un ejecutor que no publica vida no cuesta ni una llamada de más: la
    /// comprobación de vida se declara sin hacer (`comprueba_firmas` lo avisa)
    /// en vez de suponerla buena, y el paso sigue (ADR-0022 §6). Sin esto, un
    /// `describe_uno` sobre un motor sin conexiones daría error de red y el
    /// paso saldría `error` por no usar referencias contra un tercero.
    #[test]
    fn sin_vida_publicada_la_referencia_pasa_sin_comprobar() {
        let mut motor = motor_con_vidas(&[]);
        let def = DefinicionPaso::nuevo("medir", 1);
        let params = vec![("rack".to_string(), referencia("banco", "v1", "s1"))];
        assert!(motor
            .veredicto_de_las_referencias(&def, "banco", &params)
            .is_none());
    }

    /// Y un paso sin referencias no paga nada: ni una comparación ni una
    /// llamada. Es lo que mantiene el coste en cero para quien no las usa.
    #[test]
    fn un_paso_sin_referencias_no_pregunta_nada() {
        let mut motor = motor_con_vidas(&[("banco", "v1")]);
        let def = DefinicionPaso::nuevo("medir", 1);
        let params = vec![("canal".to_string(), Value::Numero(2.0))];
        assert!(motor
            .veredicto_de_las_referencias(&def, "banco", &params)
            .is_none());
    }

    /// **Criterio 4 del encargo**, en su parte decidible sin red: un ejecutor
    /// que se ha reiniciado se detecta **antes** de invocar, y el paso no mide.
    ///
    /// Visto fallar comparando `ahora` consigo mismo en vez de con la vida de
    /// la referencia: los tres casos pasan a devolver `None` y el paso saldría
    /// a medir contra un banco que ya no existe.
    #[test]
    fn un_ejecutor_reiniciado_se_detecta_antes_de_invocar() {
        let vieja = expr::Reference {
            executor: "banco".into(),
            lifetime: "v1".into(),
            payload: "s1".into(),
        };
        let refs = vec![("rack", &vieja)];

        // Misma vida: se invoca.
        assert!(veredicto_de_vida("medir", "banco", &refs, Ok("v1".into())).is_none());

        // Vida distinta: se reinició.
        let r = veredicto_de_vida("medir", "banco", &refs, Ok("v2".into()))
            .expect("una vida distinta no se invoca");
        assert_eq!(r.estado, "error");
        assert!(r.valor_medido.is_none(), "y no mide");
        assert!(
            r.mensaje.contains("reiniciado")
                && r.mensaje.contains("v1")
                && r.mensaje.contains("v2"),
            "{}",
            r.mensaje
        );

        // Y el reinicio que además se llevó la conexión por delante: no
        // contesta, y eso también es «no se invoca», no un aborto de corrida.
        let caido = veredicto_de_vida("medir", "banco", &refs, Err("conexión cerrada".into()))
            .expect("un ejecutor que no contesta no se invoca");
        assert_eq!(caido.estado, "error");
        assert!(
            caido.mensaje.contains("conexión cerrada"),
            "{}",
            caido.mensaje
        );
    }

    /// Una referencia sin vida no se puede comparar con nada, y no se inventa
    /// un veredicto sobre ella (ADR-0019, Regla 2).
    #[test]
    fn una_referencia_sin_vida_no_se_juzga() {
        let sin_vida = expr::Reference {
            executor: "banco".into(),
            lifetime: String::new(),
            payload: "s1".into(),
        };
        assert!(
            veredicto_de_vida("medir", "banco", &[("rack", &sin_vida)], Ok("v2".into())).is_none()
        );
    }
}

/// ADR-0020: los tests del contrato de parámetros y salidas. Cada uno se ha
/// visto en rojo reintroduciendo el fallo que vigila — un test de regresión
/// que no se ha visto fallar no está verificado.
#[cfg(test)]
mod tests_adr0020 {
    use super::*;
    use modelo::ValorDefinicion;

    fn paso_con_parametros() -> DefinicionPaso {
        let mut d = DefinicionPaso::nuevo("medir_voltaje", 1);
        d.entradas = Some(vec![(
            "canal".to_string(),
            EntradaPaso::Literal(ValorDefinicion::Numero(2.0)),
        )]);
        d
    }

    fn paso_que_lee_salidas() -> DefinicionPaso {
        let mut d = DefinicionPaso::nuevo("medir_voltaje", 1);
        d.asigna = Some(vec![Asignacion {
            var: "t".to_string(),
            expr: expr::parse_expresion("result.outputs.temperatura").unwrap(),
        }]);
        d
    }

    /// **El test que el ADR señala como el que importa.** Un ejecutor de
    /// contrato 1 que recibe un paso con `parametros` tiene que salir
    /// `error`, nunca `paso` ni `fallo`.
    ///
    /// Visto en rojo devolviendo `CONTRACT` en vez de `0` como eco: el paso
    /// sale `paso`, que es exactamente el verde falso que esto impide. Un
    /// test de eco que sólo recorre el camino feliz no protege de nada.
    #[test]
    fn un_ejecutor_de_contrato_1_con_parametros_es_error() {
        let r = veredicto_del_eco(&paso_con_parametros(), EMBEDIDO, 0)
            .expect("el eco insuficiente tiene que producir un veredicto");
        assert_eq!(r.estado, "error", "nunca 'fallo': no es culpa de la unidad");
        assert!(
            r.mensaje.contains("embebido"),
            "nombra el endpoint: {}",
            r.mensaje
        );
        // Un ejecutor de contrato 1 responde `0` (default de proto3), pero
        // el mensaje tiene que hablarle al usuario de contratos, no de
        // detalles de protobuf.
        assert!(
            r.mensaje.contains("contrato 1"),
            "nombra el contrato que entiende, no el 0 del cable: {}",
            r.mensaje
        );
        assert!(
            r.mensaje.contains(&CONTRACT.to_string()),
            "y el que hacía falta"
        );
    }

    /// El mismo verde falso por la otra puerta: el paso no manda parámetros,
    /// pero su `asigna` lee una salida que un ejecutor de contrato 1 no sabe
    /// devolver.
    #[test]
    fn leer_salidas_de_un_ejecutor_de_contrato_1_tambien_es_error() {
        let r = veredicto_del_eco(&paso_que_lee_salidas(), "python", 0).expect("también aquí");
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("python"));
    }

    /// Y el recíproco, que es lo que mantiene vivo todo lo escrito hasta
    /// ahora: un paso que no pide nada nuevo corre contra un ejecutor viejo
    /// exactamente igual que antes.
    #[test]
    fn un_paso_sin_parametros_sigue_valiendo_con_contrato_1() {
        let viejo = DefinicionPaso::nuevo("verificar_led", 1);
        assert!(veredicto_del_eco(&viejo, EMBEDIDO, 0).is_none());
    }

    /// El caso que estrena el contrato 3: un ejecutor que entiende el 2 —el
    /// contrato en castellano, con `parametros` y `salidas`— ya no vale para
    /// un paso que manda `inputs`. No es un detalle de nombres: los campos
    /// cambiaron de tag y de tipo, así que ese ejecutor leería basura.
    ///
    /// Visto en rojo devolviendo `CONTRACT` como eco.
    #[test]
    fn un_ejecutor_del_contrato_anterior_tampoco_vale() {
        let r = veredicto_del_eco(&paso_con_parametros(), EMBEDIDO, CONTRACT - 1)
            .expect("el contrato anterior ya no basta");
        assert_eq!(r.estado, "error");
        assert!(
            r.mensaje.contains(&(CONTRACT - 1).to_string()),
            "nombra el contrato que entiende: {}",
            r.mensaje
        );
    }

    #[test]
    fn con_el_eco_correcto_no_hay_veredicto() {
        assert!(veredicto_del_eco(&paso_con_parametros(), EMBEDIDO, CONTRACT).is_none());
    }

    /// `lee_salidas` recorre el AST entero: la lectura puede estar dentro de
    /// una operación, no sólo suelta. Visto en rojo dejando el recorrido sólo
    /// en el nodo raíz.
    #[test]
    fn una_salida_leida_dentro_de_una_operacion_tambien_cuenta() {
        let mut d = DefinicionPaso::nuevo("m", 1);
        d.asigna = Some(vec![Asignacion {
            var: "t".to_string(),
            expr: expr::parse_expresion("result.outputs.temperatura * 2 + 1").unwrap(),
        }]);
        assert!(necesita_contrato_2(&d), "está dentro de una BinOp anidada");
    }

    /// ADR-0020 §2: una expresión que falla convierte el paso en `error` y
    /// **el ejecutor no se llama**. Nunca un valor por defecto: medir con un
    /// parámetro inventado da un número que parece bueno y no lo es.
    ///
    /// Visto en rojo haciendo que `evalua_entradas` caiga a `Value::Nulo`
    /// cuando la evaluación falla.
    #[test]
    fn una_expresion_que_falla_deja_el_paso_en_error() {
        let mut d = DefinicionPaso::nuevo("medir_voltaje", 1);
        d.entradas = Some(vec![(
            "canal".to_string(),
            EntradaPaso::Expresion(expr::parse_expresion("locals.no_existe").unwrap()),
        )]);
        let def = DefinicionSecuencia::default();
        let mut ent = EntornoMotor::desde_definicion(&def);
        let r = evalua_entradas(&d, &mut ent).expect_err("no se puede evaluar");
        assert_eq!(r.estado, "error");
        assert!(
            r.mensaje.contains("canal"),
            "nombra el parámetro: {}",
            r.mensaje
        );
    }

    /// Un parámetro que evalúa a `nothing` tampoco se manda: el paso mediría
    /// sin él y nadie se enteraría.
    #[test]
    fn un_parametro_nulo_no_viaja() {
        let mut d = DefinicionPaso::nuevo("m", 1);
        d.entradas = Some(vec![(
            "canal".to_string(),
            EntradaPaso::Expresion(expr::parse_expresion("nothing").unwrap()),
        )]);
        let def = DefinicionSecuencia::default();
        let mut ent = EntornoMotor::desde_definicion(&def);
        let r = evalua_entradas(&d, &mut ent).expect_err("un nulo no puede viajar");
        assert_eq!(r.estado, "error");
    }

    /// Los literales se mandan con su tipo y en el orden que fijó el
    /// cargador, que es determinista para que dos corridas iguales produzcan
    /// los mismos bytes.
    #[test]
    fn los_literales_viajan_con_su_tipo() {
        let mut d = DefinicionPaso::nuevo("m", 1);
        d.entradas = Some(vec![
            (
                "a_canal".to_string(),
                EntradaPaso::Literal(ValorDefinicion::Numero(2.0)),
            ),
            (
                "b_etiqueta".to_string(),
                EntradaPaso::Literal(ValorDefinicion::Texto("banco-3".into())),
            ),
            (
                "c_promediar".to_string(),
                EntradaPaso::Literal(ValorDefinicion::Bool(true)),
            ),
        ]);
        let def = DefinicionSecuencia::default();
        let mut ent = EntornoMotor::desde_definicion(&def);
        let v = evalua_entradas(&d, &mut ent).unwrap();
        assert_eq!(
            v,
            vec![
                ("a_canal".to_string(), Value::Numero(2.0)),
                ("b_etiqueta".to_string(), Value::Texto("banco-3".into())),
                ("c_promediar".to_string(), Value::Bool(true)),
            ]
        );
    }
}
