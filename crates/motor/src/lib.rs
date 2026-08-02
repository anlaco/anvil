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
    Asignacion, DefinicionPaso, DefinicionSecuencia, Limite, ResultadoSecuencia, ResultadoStep,
    ResultSink, TipoPaso,
};
use prost::Message;
use wasi_grpc::grpc::Cliente;
use wasi_grpc::net;

pub use entorno::EntornoMotor;
use expr::{eval, eval_sentencias, Entorno, Expresion, Scope, Sentencia, Value};

/// El motor: un cliente gRPC contra un ejecutor de pasos.
pub struct Motor {
    cliente: Cliente,
}

/// Qué salió mal al correr una secuencia. Un paso que *falla* no es un
/// error del motor — eso es un resultado válido; esto es que la
/// comunicación se rompió.
#[derive(Debug)]
pub enum Error {
    Red(net::Error),
    Protobuf(prost::DecodeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Red(e) => write!(f, "{e}"),
            Error::Protobuf(e) => write!(f, "respuesta ilegible: {e}"),
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
    pub fn conecta(host: &str, puerto: u16) -> Result<Self, Error> {
        Ok(Motor { cliente: Cliente::conectar(host, puerto)? })
    }

    /// Invoca un paso por nombre. Cada llamada gasta un stream HTTP/2
    /// nuevo; de eso se encarga el cliente de `wasi-grpc`.
    fn ejecuta_paso(&mut self, nombre: &str, intento: i32) -> Result<ResultadoStep, Error> {
        let peticion = PeticionPaso { nombre: nombre.to_string(), intento };
        let bytes = self.cliente.unaria(RUTA_INVOCA, &peticion.encode_to_vec())?;
        Ok(ResultadoPasoProto::decode(&bytes[..])?.into())
    }

    /// Corre un paso hasta que pase o se agoten los intentos.
    /// `reintentos` es el número **total** de intentos: 1 = un solo tiro.
    fn ejecuta_con_reintentos(&mut self, def: &DefinicionPaso) -> Result<ResultadoStep, Error> {
        let max = def.reintentos.max(1);
        let mut resultado = self.ejecuta_paso(&def.nombre, 1)?;
        let mut intento = 1;
        while !resultado.paso() && intento < max {
            intento += 1;
            resultado = self.ejecuta_paso(&def.nombre, intento as i32)?;
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
        sink.on_inicio_secuencia(definicion);
        let mut secuencia = ResultadoSecuencia::nueva(&definicion.nombre);
        // El entorno de variables vive toda la secuencia: Locals persiste
        // entre pasos Main; FileGlobals se cargó al inicio.
        let mut entorno = EntornoMotor::desde_definicion(definicion);

        // --- Setup: corren todos. Un saltado no estropea el setup. ---
        let mut setup_ok = true;
        for p in &definicion.pasos_setup {
            let r = self.corre_un_paso(p, &mut entorno, sink)?;
            let fallo = !r.paso() && r.estado != "saltado";
            secuencia.registra(r.clone());
            if fallo {
                setup_ok = false;
            }
            // pause_on_fail corta el Setup (que por defecto corre todos). El
            // modo interactivo "espera input" es post-MVP.
            if p.pause_on_fail && fallo {
                break;
            }
        }

        // --- Main: solo si el Setup fue bien; corta en el primer fallo. ---
        if setup_ok {
            for p in &definicion.pasos_main {
                let r = self.corre_un_paso(p, &mut entorno, sink)?;
                let fallo = !r.paso() && r.estado != "saltado";
                secuencia.registra(r.clone());
                if fallo {
                    break;
                }
                // pause_on_fail aquí es no-op sobre el corte (Main ya corta en
                // fallo); se respeta el campo por simetría y para futuros modos.
            }
        }

        // --- Cleanup siempre: un equipo encendido es peor que una secuencia
        // que falló. pause_on_fail NO corta el Cleanup (principio "siempre"). ---
        for p in &definicion.pasos_cleanup {
            let r = self.corre_un_paso(p, &mut entorno, sink)?;
            secuencia.registra(r.clone());
        }

        sink.on_fin_secuencia(&secuencia);
        Ok(secuencia)
    }

    /// Corre un solo paso (Setup/Main/Cleanup comparten esta lógica): disable,
    /// precondición, invocación (Grpc o statement local), asigna y lifecycle
    /// del sink. Devuelve el `ResultadoStep` a registrar.
    fn corre_un_paso(
        &mut self,
        p: &DefinicionPaso,
        ent: &mut EntornoMotor,
        sink: &mut impl ResultSink,
    ) -> Result<ResultadoStep, Error> {
        sink.on_inicio_paso(p);

        // (a) disable: se salta sin invocar ni evaluar nada.
        if p.disable {
            let r = ResultadoStep::nuevo(&p.nombre, "saltado", "disable");
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
                    sink.on_resultado(&r);
                    sink.on_fin_paso(p);
                    return Ok(r);
                }
            }
        }

        // (c)/(d) según tipo de paso (RF-27).
        let mut r = match p.tipo {
            TipoPaso::Statement => ejecuta_statement_puro(p.statement.as_deref(), &p.nombre, ent),
            TipoPaso::Grpc => self.ejecuta_con_reintentos(p)?,
        };

        // (e) asigna (RF-31): sólo tras un paso Grpc, vuelca campos de
        // `resultado` a Locals. Un statement asigna dentro de su sentencia.
        if matches!(p.tipo, TipoPaso::Grpc) {
            if let Some(asignaciones) = &p.asigna {
                r = aplica_asigna(asignaciones, r, ent);
            }
        }

        sink.on_resultado(&r);
        sink.on_fin_paso(p);
        Ok(r)
    }
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
        Ok(Value::Bool(false)) => {
            VeredictoPre::Salta(ResultadoStep::nuevo(nombre, "saltado", "precondición falsa"))
        }
        Ok(v) => VeredictoPre::Salta(ResultadoStep::nuevo(
            nombre,
            "error",
            format!("precondición: se esperaba bool, no {}", v.tipo()),
        )),
        Err(e) => VeredictoPre::Salta(ResultadoStep::nuevo(nombre, "error", format!("precondición: {e}"))),
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

/// Aplica las `asignaciones` (RF-31): vuelca cada `expr` (sobre `resultado`/
/// scopes) a una Local. Pura (sin red). Si una asignación falla al evaluar o
/// al escribir, convierte el paso a `"error"` (es un fallo de definición) y
/// añade el detalle al mensaje, preservando el mensaje original del paso.
fn aplica_asigna(asignaciones: &[Asignacion], mut r: ResultadoStep, ent: &mut EntornoMotor) -> ResultadoStep {
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
        assert_eq!(r.mensaje, "medido", "si pasa, el mensaje del paso se respeta");
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
            Limite::Comparacion { op: Operador::Ge, esperado: 1000.0 },
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
        assert_eq!(r.estado, "fallo", "el paso ya falló: el límite no lo mejora");
        assert_eq!(r.limite_min, Some(4.5), "pero sí rellena el límite para el reporte");
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
        assert!(matches!(evalua_precondicion(&pre, &mut env, "p"), VeredictoPre::Continua));
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
        assert_eq!(r.estado, "error", "una asigna que falla es un fallo de definición");
        assert!(r.mensaje.contains("asigna"));
    }
}
