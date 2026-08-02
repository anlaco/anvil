//! El modelo de datos del secuenciador y los mensajes de `paso.proto`:
//! mismos campos, mismos estados, mismo contrato en el cable.

use std::collections::HashMap;

pub mod proto;
pub mod result_sink;
pub use result_sink::{ResultSink, SinkCompuesto};

/// Un operador de comparación para un `Limite::Comparacion`.
///
/// Vive en el modelo (datos), no en `paso.proto`: el límite no viaja por el
/// cable ([contrato-grpc.md](../../docs/contrato-grpc.md)). Lo declara la
/// secuencia en YAML y lo evalúa el motor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operador {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Operador {
    /// Símbolo del operador para mostrarlo en mensajes y reportes
    /// (`"="`, `"!="`, `"<"`, `"<="`, `">"`, `">="`).
    pub fn simbolo(&self) -> &'static str {
        match self {
            Operador::Eq => "=",
            Operador::Ne => "!=",
            Operador::Lt => "<",
            Operador::Le => "<=",
            Operador::Gt => ">",
            Operador::Ge => ">=",
        }
    }

    /// Parsea un operador desde el texto del YAML (`eq`/`ne`/`lt`/`le`/`gt`/
    /// `ge`). Devuelve `None` si no es uno de los seis — el cargador lo
    /// convierte en error de validación.
    pub fn de_texto(s: &str) -> Option<Self> {
        match s.trim() {
            "eq" => Some(Operador::Eq),
            "ne" => Some(Operador::Ne),
            "lt" => Some(Operador::Lt),
            "le" => Some(Operador::Le),
            "gt" => Some(Operador::Gt),
            "ge" => Some(Operador::Ge),
            _ => None,
        }
    }

    /// Aplica el operador a dos valores. Es lo que `Limite::evalua` usa para
    /// `Comparacion`.
    fn aplica(&self, valor: f64, esperado: f64) -> bool {
        match self {
            Operador::Eq => valor == esperado,
            Operador::Ne => valor != esperado,
            Operador::Lt => valor < esperado,
            Operador::Le => valor <= esperado,
            Operador::Gt => valor > esperado,
            Operador::Ge => valor >= esperado,
        }
    }
}

/// Un límite como **dato first-class** (RF-29): una regla de aceptación que la
/// secuencia declara en YAML y el motor evalúa contra la medida que devuelve
/// el paso. El paso **no conoce el umbral**; solo mide.
///
/// Esto es lo que separa el *qué es aceptable* (datos, cambia en producción)
/// del *cómo se mide* (código del paso). Ver
/// [limites-y-estados.md](../../docs/diseno/limites-y-estados.md) y
/// ADR-0008.
#[derive(Debug, Clone, PartialEq)]
pub enum Limite {
    /// Rango inclusivo high/low: `min <= valor <= max` → `paso`; si no, `fallo`.
    Rango { min: f64, max: f64 },
    /// Comparación contra un valor esperado: `valor {op} esperado` →
    /// `paso`; si no, `fallo`.
    Comparacion { op: Operador, esperado: f64 },
}

impl Limite {
    /// Evalúa un valor contra el límite → `"paso"` o `"fallo"`. Lógica pura,
    /// sin gRPC ni IO: el motor la reutiliza, los tests la prueban directa.
    pub fn evalua(&self, valor: f64) -> &'static str {
        match self {
            Limite::Rango { min, max } => {
                if valor >= *min && valor <= *max {
                    "paso"
                } else {
                    "fallo"
                }
            }
            Limite::Comparacion { op, esperado } => {
                if op.aplica(valor, *esperado) {
                    "paso"
                } else {
                    "fallo"
                }
            }
        }
    }
}

/// Lo que un paso YA corrido devolvió.
///
/// `estado` es uno de `"paso"`, `"fallo"` o `"error"` — se mantiene como
/// texto (y no como enum) porque viaja así en `paso.proto` y porque el
/// contrato admite pasos escritos en cualquier lenguaje.
///
/// `valor_esperado` y `operador` describen un `Limite::Comparacion` aplicado
/// por el motor. **No** viajan en `paso.proto`: los rellena el motor tras la
/// invocación a partir del límite del YAML (ADR-0008); el `ResultadoStep`
/// enriquecido solo va a los sinks, no vuelve al cable.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoStep {
    pub nombre: String,
    pub estado: String,
    pub mensaje: String,
    pub valor_medido: Option<f64>,
    pub limite_min: Option<f64>,
    pub limite_max: Option<f64>,
    /// Valor esperado de un `Limite::Comparacion` aplicado por el motor.
    /// `None` si el límite es un rango o si no hay límite. No viaja en
    /// `paso.proto` (ADR-0008).
    pub valor_esperado: Option<f64>,
    /// Operador del `Limite::Comparacion` aplicado por el motor. `None` si
    /// el límite es un rango o si no hay límite. No viaja en `paso.proto`.
    pub operador: Option<Operador>,
    /// Sub-pasos anidados de un **sequence call** (M4b, RF-27): el
    /// `ResultadoStep` de la llamada lleva, anidados, los resultados de la
    /// subsecuencia. `None` para cualquier otro tipo de paso. No viaja en
    /// `paso.proto` (sequence call es motor-side, ADR-0010).
    pub sub_pasos: Option<Vec<ResultadoStep>>,
}

impl ResultadoStep {
    /// Un resultado sin medida asociada (el caso común).
    pub fn nuevo(nombre: &str, estado: &str, mensaje: impl Into<String>) -> Self {
        ResultadoStep {
            nombre: nombre.to_string(),
            estado: estado.to_string(),
            mensaje: mensaje.into(),
            valor_medido: None,
            limite_min: None,
            limite_max: None,
            valor_esperado: None,
            operador: None,
            sub_pasos: None,
        }
    }

    /// Un resultado con medida y límites high/low (p. ej. una medida de
    /// voltaje cuyo paso ya conoce el umbral). Hoy solo lo usan los tests y
    /// los pasos demo legacy; en M3 el flujo normal es `medido_valor` +
    /// límite evaluado por el motor.
    pub fn medido(
        nombre: &str,
        estado: &str,
        mensaje: impl Into<String>,
        valor: f64,
        min: f64,
        max: f64,
    ) -> Self {
        ResultadoStep {
            valor_medido: Some(valor),
            limite_min: Some(min),
            limite_max: Some(max),
            ..ResultadoStep::nuevo(nombre, estado, mensaje)
        }
    }

    /// Un resultado con **medida pero sin umbral**: el paso midió un valor y
    /// no conoce el límite. Es lo que devuelve un paso de *limit test* en M3:
    /// el motor evalúa el `Limite` del YAML contra `valor_medido` y produce el
    /// estado final (ADR-0008). El estado que trae aquí es el de la medición
    /// (`paso` = medí bien; `error` = no pude medir).
    pub fn medido_valor(
        nombre: &str,
        estado: &str,
        mensaje: impl Into<String>,
        valor: f64,
    ) -> Self {
        ResultadoStep {
            valor_medido: Some(valor),
            ..ResultadoStep::nuevo(nombre, estado, mensaje)
        }
    }

    pub fn paso(&self) -> bool {
        self.estado == "paso"
    }
}

/// El resultado agregado de una secuencia corrida.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultadoSecuencia {
    pub nombre: String,
    pub pasos: Vec<ResultadoStep>,
}

impl ResultadoSecuencia {
    pub fn nueva(nombre: &str) -> Self {
        ResultadoSecuencia { nombre: nombre.to_string(), pasos: Vec::new() }
    }

    /// Añade un resultado de paso al agregado de la secuencia.
    pub fn registra(&mut self, paso: ResultadoStep) {
        self.pasos.push(paso);
    }

    /// Estado agregado de la secuencia. Un `error` en cualquier paso manda
    /// sobre un `fallo`; sin ninguno de los dos, la secuencia pasa.
    pub fn estado(&self) -> &'static str {
        if self.pasos.iter().any(|p| p.estado == "error") {
            "error"
        } else if self.pasos.iter().any(|p| p.estado == "fallo") {
            "fallo"
        } else {
            "paso"
        }
    }

    /// Reporte de la secuencia en texto. El formato es parte de la spec
    /// (RNF-08): no se toca sin querer tocar la especificación.
    ///
    /// Escribe al `Write` que se le pase, para que el sink de consola (y
    /// los tests) no se acoplen a stdout. `reporte()` (la API pública
    /// congelada) delega aquí con `stdout` y produce los mismos bytes que
    /// el `println!` original.
    pub fn reporte_a(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(w, "=== {}: {} ===", self.nombre, self.estado())?;
        for p in &self.pasos {
            Self::escribe_paso(w, p, 1)?;
        }
        Ok(())
    }

    /// Escribe un `ResultadoStep` (y, recursivamente, sus `sub_pasos`) al
    /// reporte textual. `nivel` es la profundidad de indentación (1 para
    /// los pasos top-level = 2 espacios, como el formato congelado de M0;
    /// 2+ para sub-pasos de un sequence call). Extensión aditiva de RNF-08:
    /// un paso sin `sub_pasos` produce exactamente la misma línea que antes.
    fn escribe_paso(w: &mut impl std::io::Write, p: &ResultadoStep, nivel: usize) -> std::io::Result<()> {
        let indent = "  ".repeat(nivel);
        writeln!(w, "{indent}[{}] {}: {}", p.estado, p.nombre, p.mensaje)?;
        if let Some(sub) = &p.sub_pasos {
            for sp in sub {
                Self::escribe_paso(w, sp, nivel + 1)?;
            }
        }
        Ok(())
    }

    /// Reporte de la secuencia a stdout. La API pública congelada (RNF-08):
    /// delega en `reporte_a` con `stdout` y traga el error de IO, igual
    /// que hacía el `println!` original.
    pub fn reporte(&self) {
        let _ = self.reporte_a(&mut std::io::stdout());
    }
}

/// El tipo de un paso. Por defecto es gRPC (el flujo de M3); `Statement`
/// es un paso **local** (RF-27) que el motor ejecuta evaluando una sentencia
/// del lenguaje de expresiones, **sin** ir por el cable. `SequenceCall`
/// (M4b, RF-27) invoca otra secuencia como un paso — también motor-side, sin
/// gRPC — anidando su `ResultadoSecuencia` en el resultado del paso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TipoPaso {
    /// El motor invoca el paso por gRPC contra el ejecutor, por nombre (M3).
    #[default]
    Grpc,
    /// Paso local (RF-27): el motor evalúa `statement` contra su entorno, sin
    /// gRPC. Útil para inicializar variables o cablear datos entre pasos.
    Statement,
    /// Invoca otra secuencia como un paso (M4b, RF-27). El motor orquesta la
    /// subsecuencia contra su propio entorno, sin gRPC; `paso.proto` no
    /// cambia (ADR-0010). El resultado se anida en `ResultadoStep.sub_pasos`.
    SequenceCall,
}

/// Una asignación declarada en el YAML (`asigna`): vuelca el resultado de
/// evaluar `expr` a una **Local** de la secuencia (la regla "sólo se muta
/// Locals" la hace valer el entorno en runtime). `var` es el nombre de la
/// Local destino, sin prefijo `locals.` (lo aporta el motor al escribir).
#[derive(Debug, Clone, PartialEq)]
pub struct Asignacion {
    pub var: String,
    pub expr: expr::Expresion,
}

/// Un argumento de un **sequence call** (M4b, RF-27): mapea un `Parameter`
/// de la subsecuencia a una **variable local del padre** (`locals.X`). Es
/// **by-reference** (como TestStand): al iniciar la subsecuencia, el motor
/// copia `locals.X` → `parameters.param`; al volver, copia
/// `parameters.param` (final) → `locals.X`. Un mismo `Parameter` es
/// entrada y salida.
///
/// `origen` debe ser una `Expresion::Var { scope: Locals, campo }` (un
/// lvalue local puro); el cargador lo valida al cargar. El motor lo lee
/// para la entrada y escribe en el mismo `campo` para la salida. Ver
/// [variables-y-alcances.md](../../docs/diseno/variables-y-alcances.md) y
/// ADR-0010.
#[derive(Debug, Clone, PartialEq)]
pub struct Argumento {
    /// Nombre del `Parameter` de la subsecuencia (la clave en
    /// `DefinicionSecuencia.parameters` de la hija).
    pub param: String,
    /// Lvalue del padre: `Expresion::Var { scope: Locals, campo }`.
    pub origen: expr::Expresion,
}

/// El valor literal de una variable declarada en el YAML (scopes
/// `locals`/`parameters`/`file_globals`). El tipo se infiere del escalar YAML:
/// número, texto o booleano. Sin árbol de propiedades tipado recursivo de
/// TestStand en el MVP (ver [variables-y-alcances.md](../../docs/diseno/variables-y-alcances.md)).
#[derive(Debug, Clone, PartialEq)]
pub enum ValorDefinicion {
    Numero(f64),
    Texto(String),
    Bool(bool),
}

impl ValorDefinicion {
    /// Traduce un literal declarado en el YAML al `Value` del expression engine
    /// (`expr::Value`). Lo usa el motor al materializar el entorno al inicio
    /// de la secuencia.
    pub fn a_value(&self) -> expr::Value {
        match self {
            ValorDefinicion::Numero(x) => expr::Value::Numero(*x),
            ValorDefinicion::Texto(s) => expr::Value::Texto(s.clone()),
            ValorDefinicion::Bool(b) => expr::Value::Bool(*b),
        }
    }
}

/// Los datos que describen QUÉ correr — a diferencia de `ResultadoStep`,
/// que es lo que un paso ya corrido devolvió.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinicionPaso {
    pub nombre: String,
    /// Número máximo de intentos (1 = sin reintentos).
    pub reintentos: u32,
    /// Regla de aceptación opcional declarada en la secuencia (RF-29). Si la
    /// hay, el motor la evalúa contra `valor_medido` tras la invocación y
    /// produce el estado final; el paso no conoce el umbral (ADR-0008).
    /// `None` = el paso decide por sí mismo (pass/fail, action sin límite).
    pub limite: Option<Limite>,
    /// RF-34: si `true`, el motor registra el paso como saltado (`"saltado"`)
    /// sin invocarlo. Default `false`.
    pub disable: bool,
    /// RF-34: si `true` y el paso falla, el motor detiene la fase en curso
    /// (en Main refuerza el corte en primer fallo; en Setup/Cleanup la corta).
    /// Default `false`. El modo interactivo "espera input" es post-MVP.
    pub pause_on_fail: bool,
    /// RF-33: precondición evaluada por el motor **antes** de invocar el
    /// paso. Si es falsa, el paso se salta sin gastar intento. AST parseado
    /// por el cargador al cargar (fail-fast). `None` = siempre corre.
    pub precondicion: Option<expr::Expresion>,
    /// RF-31: asignaciones tras el paso (sólo `Grpc`), para volcar campos de
    /// `resultado` a `Locals`. ASTs parseados al cargar. `None` = sin asignar.
    pub asigna: Option<Vec<Asignacion>>,
    /// RF-27: tipo de paso. Default `Grpc` (preserva compat con M3).
    pub tipo: TipoPaso,
    /// RF-27: sentencias a ejecutar si `tipo == Statement` (paso local, sin
    /// gRPC). `None` si `Grpc`. El cargador valida la coherencia con `tipo`.
    pub statement: Option<Vec<expr::Sentencia>>,
    /// M4b/RF-27: destino de la subsecuencia si `tipo == SequenceCall`. Es
    /// un **nombre** (subsecuencia inline del mismo archivo) o un **path
    /// relativo** (archivo externo); el cargador distingue por la
    /// convención `es_path` (ver `formato-de-secuencia.md`). `None` si no
    /// es `SequenceCall`.
    pub secuencia: Option<String>,
    /// M4b/RF-27: argumentos by-reference del sequence call (`locals.X`
    /// ↔ `parameters.param`). `None` si no es `SequenceCall`.
    pub parametros: Option<Vec<Argumento>>,
}

impl DefinicionPaso {
    pub fn nuevo(nombre: &str, reintentos: u32) -> Self {
        DefinicionPaso {
            nombre: nombre.to_string(),
            reintentos,
            limite: None,
            disable: false,
            pause_on_fail: false,
            precondicion: None,
            asigna: None,
            tipo: TipoPaso::Grpc,
            statement: None,
            secuencia: None,
            parametros: None,
        }
    }

    /// Como `nuevo` pero fijando un límite. Lo usa el cargador al traducir el
    /// YAML (límite embebido) y el property loader (sidecar).
    pub fn con_limite(nombre: &str, reintentos: u32, limite: Limite) -> Self {
        DefinicionPaso {
            limite: Some(limite),
            ..DefinicionPaso::nuevo(nombre, reintentos)
        }
    }
}

/// Una secuencia como datos: el motor la recorre sin saber qué hace cada
/// paso, invocándolos por gRPC por nombre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefinicionSecuencia {
    pub nombre: String,
    pub pasos_setup: Vec<DefinicionPaso>,
    pub pasos_main: Vec<DefinicionPaso>,
    pub pasos_cleanup: Vec<DefinicionPaso>,
    /// RF-31: variables locales de la secuencia, mutables por `asigna` durante
    /// la ejecución. Materializadas por el motor al iniciar la secuencia.
    pub locals: HashMap<String, ValorDefinicion>,
    /// RF-31: parámetros de entrada/salida. En M4-núcleo (sin sequence call)
    /// están vacíos y reservados para M4b.
    pub parameters: HashMap<String, ValorDefinicion>,
    /// RF-31: globales del archivo, compartidas por todas las secuencias del
    /// archivo. Inmutables durante la ejecución de un paso.
    pub file_globals: HashMap<String, ValorDefinicion>,
    /// M4b/RF-27: subsecuencias declaradas **inline** en el mismo archivo,
    /// invocables por nombre desde cualquier secuencia de ese archivo.
    /// **Privadas del archivo**: no se exponen a otros archivos (ésos
    /// invocan la secuencia raíz por path). El cargador las resuelve al
    /// cargar; el motor las lee por nombre en `def.subsecuencias`.
    pub subsecuencias: HashMap<String, DefinicionSecuencia>,
}

/// Un programa Anvil (M4b): la secuencia raíz a ejecutar más las
/// subsecuencias de **archivos externos** cargados, keyed por path
/// normalizado. Las subsecuencias **inline** no viven aquí: viven dentro
/// de la `DefinicionSecuencia` que las declara (campo `subsecuencias`).
///
/// El cargador lo construye (resolución de paths + validación + detección
/// de ciclos); el motor lo recorre **sin abrir ficheros** (ADR-0005). Es
/// puros datos: no sabe de YAML ni de `std::fs`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Programa {
    /// La secuencia a ejecutar (la del `nombre:` del archivo de entrada).
    pub raiz: DefinicionSecuencia,
    /// Subsecuencias de archivos externos, keyed por path normalizado. El
    /// valor es la secuencia **raíz** de ese archivo; las inline de cada
    /// archivo viven dentro de su `DefinicionSecuencia`.
    pub archivos: HashMap<String, DefinicionSecuencia>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_agregado() {
        let mut s = ResultadoSecuencia::nueva("s");
        assert_eq!(s.estado(), "paso", "una secuencia vacía pasa");

        s.registra(ResultadoStep::nuevo("a", "paso", "ok"));
        assert_eq!(s.estado(), "paso");

        s.registra(ResultadoStep::nuevo("b", "fallo", "mal"));
        assert_eq!(s.estado(), "fallo");

        // error manda sobre fallo, aunque llegue después.
        s.registra(ResultadoStep::nuevo("c", "error", "peor"));
        assert_eq!(s.estado(), "error");
    }

    #[test]
    fn error_manda_aunque_llegue_antes() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "error", "peor"));
        s.registra(ResultadoStep::nuevo("b", "fallo", "mal"));
        assert_eq!(s.estado(), "error");
    }

    /// RNF-08: el formato textual de `reporte_a` es spec congelada.
    /// Este test congela los bytes exactos para detectar cambios
    /// accidentales.
    #[test]
    fn reporte_a_congela_el_formato() {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::medido("medir_voltaje", "fallo", "voltaje fuera de rango", 4.2, 4.5, 5.5));
        s.registra(ResultadoStep::nuevo("verificar_led", "paso", "led encendido"));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== basica: fallo ===
  [fallo] medir_voltaje: voltaje fuera de rango
  [paso] verificar_led: led encendido
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }

    #[test]
    fn limite_rango_dentro_fuera_y_fronteras() {
        let r = Limite::Rango { min: 4.5, max: 5.5 };
        assert_eq!(r.evalua(5.0), "paso", "dentro del rango");
        assert_eq!(r.evalua(4.2), "fallo", "por debajo");
        assert_eq!(r.evalua(6.0), "fallo", "por encima");
        // Fronteras inclusivas.
        assert_eq!(r.evalua(4.5), "paso", "min incluido");
        assert_eq!(r.evalua(5.5), "paso", "max incluido");
    }

    #[test]
    fn limite_comparacion_cubre_seis_operadores() {
        use Operador::*;
        assert_eq!(Limite::Comparacion { op: Eq, esperado: 1000.0 }.evalua(1000.0), "paso");
        assert_eq!(Limite::Comparacion { op: Eq, esperado: 1000.0 }.evalua(999.0), "fallo");
        assert_eq!(Limite::Comparacion { op: Ne, esperado: 1000.0 }.evalua(999.0), "paso");
        assert_eq!(Limite::Comparacion { op: Lt, esperado: 1000.0 }.evalua(999.0), "paso");
        assert_eq!(Limite::Comparacion { op: Lt, esperado: 1000.0 }.evalua(1000.0), "fallo", "lt excluye el igual");
        assert_eq!(Limite::Comparacion { op: Le, esperado: 1000.0 }.evalua(1000.0), "paso", "le incluye el igual");
        assert_eq!(Limite::Comparacion { op: Gt, esperado: 1000.0 }.evalua(1001.0), "paso");
        assert_eq!(Limite::Comparacion { op: Ge, esperado: 1000.0 }.evalua(1000.0), "paso");
    }

    #[test]
    fn operador_simbolo_y_parseo_ida_y_vuelta() {
        for op in [Operador::Eq, Operador::Ne, Operador::Lt, Operador::Le, Operador::Gt, Operador::Ge] {
            let texto = match op {
                Operador::Eq => "eq",
                Operador::Ne => "ne",
                Operador::Lt => "lt",
                Operador::Le => "le",
                Operador::Gt => "gt",
                Operador::Ge => "ge",
            };
            assert_eq!(Operador::de_texto(texto), Some(op), "parseo de {texto}");
        }
        assert_eq!(Operador::de_texto("no_existe"), None);
        assert_eq!(Operador::de_texto("  eq  "), Some(Operador::Eq), "tolera espacios");
        // Símbolos conocidos para el reporte.
        assert_eq!(Operador::Ge.simbolo(), ">=");
        assert_eq!(Operador::Ne.simbolo(), "!=");
    }

    #[test]
    fn definicion_paso_con_limite_lo_guarda() {
        let p = DefinicionPaso::con_limite("medir_voltaje", 1, Limite::Rango { min: 4.5, max: 5.5 });
        assert_eq!(p.limite, Some(Limite::Rango { min: 4.5, max: 5.5 }));
        // Sin límite por defecto: el paso decide (pass/fail, action).
        assert_eq!(DefinicionPaso::nuevo("verificar_led", 1).limite, None);
    }

    #[test]
    fn medido_valor_deja_limites_vacios() {
        // Un paso que mide sin conocer el umbral: el motor rellena los
        // campos de límite después, desde el YAML (ADR-0008).
        let r = ResultadoStep::medido_valor("medir_voltaje", "paso", "medido: 4.2 V", 4.2);
        assert_eq!(r.valor_medido, Some(4.2));
        assert_eq!(r.limite_min, None);
        assert_eq!(r.limite_max, None);
        assert_eq!(r.valor_esperado, None);
        assert_eq!(r.operador, None);
    }

    /// M4: un paso saltado (disable / precondición falsa) es **neutral** en el
    /// agregado — no cuenta como fallo ni como error. Una secuencia con sólo
    /// pasos saltados pasa.
    #[test]
    fn saltado_no_cuenta_como_fallo_ni_error() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "saltado", "disable"));
        assert_eq!(s.estado(), "paso");
        // Un saltado junto a un fallo: manda el fallo, no se anula.
        s.registra(ResultadoStep::nuevo("b", "fallo", "mal"));
        assert_eq!(s.estado(), "fallo");
    }

    /// RNF-08 (extensión aditiva de M4): el estado `"saltado"` aparece en el
    /// reporte textual congelado. El formato de línea no cambia; sólo se
    /// añade un nuevo *valor* de estado.
    #[test]
    fn reporte_incluye_estado_saltado() {
        let mut s = ResultadoSecuencia::nueva("variables");
        s.registra(ResultadoStep::nuevo("init_log", "paso", "statement ok"));
        s.registra(ResultadoStep::nuevo("paso_obsoleto", "saltado", "disable"));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== variables: paso ===
  [paso] init_log: statement ok
  [saltado] paso_obsoleto: disable
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }

    /// Los defaults de `DefinicionPaso::nuevo` preservan el comportamiento de
    /// M3 (disable=false, pause_on_fail=false, sin precondición/asigna, Grpc).
    #[test]
    fn definicion_paso_nuevo_tiene_defaults_de_m4() {
        let p = DefinicionPaso::nuevo("verificar_led", 1);
        assert!(!p.disable);
        assert!(!p.pause_on_fail);
        assert_eq!(p.precondicion, None);
        assert_eq!(p.asigna, None);
        assert_eq!(p.tipo, TipoPaso::Grpc);
        assert_eq!(p.statement, None);
        // M4b: los campos de sequence call también parten de None.
        assert_eq!(p.secuencia, None);
        assert_eq!(p.parametros, None);
    }

    /// M4b: un `ResultadoStep` nuevo (incluido `medido_valor`) parte con
    /// `sub_pasos: None`; un sequence call lo rellena con `Some(...)`.
    #[test]
    fn resultado_step_nuevo_tiene_sub_pasos_none() {
        let r = ResultadoStep::nuevo("p", "paso", "ok");
        assert_eq!(r.sub_pasos, None);
        let m = ResultadoStep::medido_valor("m", "paso", "ok", 4.2);
        assert_eq!(m.sub_pasos, None);
    }

    /// M4b: el estado agregado de un sequence call es el de la subsecuencia
    /// (se anida en `sub_pasos`); el agregado de la secuencia padre mira
    /// `p.estado` del call (que ya es el agregado), sin descender.
    #[test]
    fn estado_agregado_con_sequence_call_anidado() {
        let mut call = ResultadoStep::nuevo("test_fuentes", "fallo", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "paso", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fallo", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        // El call ya trae "fallo" (agregado de la sub); la secuencia padre
        // lo ve sin descender a sub_pasos.
        assert_eq!(s.estado(), "fallo");
    }

    /// RNF-08 (extensión aditiva de M4b): el reporte textual anida los
    /// sub-pasos de un sequence call con +2 espacios por nivel. Los pasos
    /// sin `sub_pasos` siguen produciendo la misma línea de siempre.
    #[test]
    fn reporte_anida_sub_pasos() {
        let mut call = ResultadoStep::nuevo("test_fuentes", "fallo", "sequence call './medir_fuentes.yaml' → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "paso", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fallo", "fuera de rango"),
            ResultadoStep::nuevo("desconectar", "paso", "ok"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== basica: fallo ===
  [fallo] test_fuentes: sequence call './medir_fuentes.yaml' → fallo
    [paso] medir_canal_1: ok
    [fallo] medir_canal_2: fuera de rango
    [paso] desconectar: ok
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }

    /// M4b: un `DefinicionSecuencia` por defecto tiene `subsecuencias`
    /// vacío y un `Programa` por defecto tiene la raíz vacía y sin archivos.
    #[test]
    fn programa_y_subsecuencias_tienen_defaults_vacios() {
        let d = DefinicionSecuencia::default();
        assert!(d.subsecuencias.is_empty());
        let p = Programa::default();
        assert!(p.raiz.pasos_main.is_empty());
        assert!(p.archivos.is_empty());
    }
}
