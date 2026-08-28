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
    /// Evalúa un valor contra el límite → `"pass"` o `"fail"`. Lógica pura,
    /// sin gRPC ni IO: el motor la reutiliza, los tests la prueban directa.
    pub fn evalua(&self, valor: f64) -> &'static str {
        match self {
            Limite::Rango { min, max } => {
                if valor >= *min && valor <= *max {
                    "pass"
                } else {
                    "fail"
                }
            }
            Limite::Comparacion { op, esperado } => {
                if op.aplica(valor, *esperado) {
                    "pass"
                } else {
                    "fail"
                }
            }
        }
    }
}

/// Los estados que un **ejecutor** puede devolver en `ResultadoStep.estado`, y
/// los únicos (ADR-0019, Regla 2, issue #28). Cualquier otra cadena la
/// convierte el motor en `"error"`: un estado que Anvil no entiende no dice
/// nada sobre la unidad.
///
/// `"inconclusive"` **no** está aquí a propósito: lo produce el motor al agregar
/// una secuencia, y un ejecutor que lo devuelva cae bajo la misma regla que
/// cualquier otro valor no reconocido (ADR-0019, «Recortes»).
pub const ESTADOS_DE_EJECUTOR: [&str; 4] = ["pass", "fail", "error", "skipped"];

/// Los campos que expone `resultado.*` a una expresión `asigna`, y los únicos
/// (ADR-0019, regla de detección, issue #27). Son tres y conocidos, así que un
/// `resultado.valor_meddio` es un typo comprobable **sin ejecutar**: lo rechaza
/// el cargador (y por tanto `--validate`), no la unidad en el banco.
pub const CAMPOS_RESULTADO: [&str; 3] = ["status", "message", "measured_value"];

/// La severidad de un estado, para agregar el veredicto de una secuencia
/// (ADR-0019, Regla 1).
///
/// **El orden de declaración *es* la severidad**: `paso < inconcluso < fallo <
/// error`. Es el modelo de OpenTAP, donde la severidad tampoco es una
/// convención de la documentación sino el valor entero del enum `Verdict`, y
/// la agregación es una comparación pura.
///
/// `saltado` **no está en la escala**: es neutral (RF-33/34) y por eso mapea a
/// `Paso`, que es el mínimo. No significa que un paso saltado haya pasado —
/// significa que no mueve el veredicto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Severidad {
    #[default]
    Paso,
    /// Anvil no pudo juzgar. **Lo produce el motor al agregar, y sólo él**: no
    /// se puede escribir en una secuencia ni devolver desde un ejecutor
    /// (ADR-0019, «Recortes»).
    Inconcluso,
    /// El DUT no cumple el criterio. Información sobre el mundo físico.
    Fallo,
    /// Anvil no pudo juzgar porque algo se rompió. Información sobre el banco.
    Error,
}

impl Severidad {
    /// La severidad de un estado de paso.
    ///
    /// Un estado **no reconocido** es `Error` (ADR-0019, Regla 2, issue #28):
    /// que un ejecutor escriba `"Paso"` con mayúscula no dice nada sobre la
    /// unidad, así que Anvil no puede juzgarla. `saltado` sí es neutral
    /// (RF-33/34) y por eso tiene rama propia: comparte destino con `paso` pero
    /// no motivo.
    ///
    /// Esta es la última red, no el diagnóstico: el motor normaliza el estado
    /// que devuelve un ejecutor en cuanto lo recibe
    /// (`motor::normaliza_estado_de_ejecutor`), y ahí es donde se nombra el
    /// valor recibido. Si algo llega hasta aquí sin reconocer, se cuenta como
    /// `Error` en vez de colarse como verde.
    pub fn de(estado: &str) -> Severidad {
        match estado {
            "error" => Severidad::Error,
            "fail" => Severidad::Fallo,
            "inconclusive" => Severidad::Inconcluso,
            // Neutrales: `paso` afirma, `saltado` no mueve el veredicto.
            "pass" | "skipped" => Severidad::Paso,
            // Lo que no se reconoce no se juzga (Regla 2).
            _ => Severidad::Error,
        }
    }

    /// La severidad como estado agregado: `"pass"`, `"inconclusive"`, `"fail"`,
    /// `"error"`. Nunca `"skipped"`: una secuencia no se salta.
    pub fn como_texto(&self) -> &'static str {
        match self {
            Severidad::Paso => "pass",
            Severidad::Inconcluso => "inconclusive",
            Severidad::Fallo => "fail",
            Severidad::Error => "error",
        }
    }
}

/// La fase de la secuencia en la que corrió un paso: Setup → Main → Cleanup.
///
/// No viaja en `paso.proto` — la sella el **motor**, que es quien conoce la
/// fase en curso, antes de entregar el `ResultadoStep` a los sinks (misma
/// pauta que `valor_esperado`/`operador` bajo ADR-0008). Distinguirla importa
/// al post-procesar: un fallo de Setup (no se pudo ni conectar el DUT), uno de
/// Main (el DUT falló el test) y uno de Cleanup (el equipo pudo quedar en un
/// estado no seguro) tienen respuestas operativas distintas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fase {
    Setup,
    #[default]
    Main,
    Cleanup,
}

impl Fase {
    /// La fase como la emiten los sinks: `"setup"`, `"main"`, `"cleanup"`.
    pub fn como_texto(&self) -> &'static str {
        match self {
            Fase::Setup => "setup",
            Fase::Main => "main",
            Fase::Cleanup => "cleanup",
        }
    }
}

/// Lo que un paso YA corrido devolvió.
///
/// `estado` es uno de `"pass"`, `"fail"`, `"error"` o `"skipped"` (este
/// último lo pone el motor, no el ejecutor) — se mantiene como texto (y no
/// como enum) porque viaja así en `paso.proto` y porque el contrato admite
/// pasos escritos en cualquier lenguaje. **Nunca `"inconclusive"`**: ése sólo
/// existe como agregado de una secuencia (ADR-0019). Ver `Severidad` para lo
/// que cada uno pesa en el veredicto.
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
    /// Fase de la secuencia en la que corrió el paso. La sella el motor antes
    /// de emitir el resultado al sink; por defecto `Main`, que es lo que vale
    /// para un resultado construido fuera del motor (tests, pasos demo). En un
    /// **sequence call**, el paso de la llamada lleva la fase del padre y cada
    /// sub-paso la suya dentro de la subsecuencia. No viaja en `paso.proto`.
    pub fase: Fase,
    /// Los parámetros que el motor **envió** a este paso, ya evaluados
    /// (ADR-0020). No vuelven del cable: los sella el motor tras la
    /// invocación, para que queden en el informe.
    ///
    /// Es lo que arregla el agujero que abrió este ADR: hasta ahora dos
    /// corridas de la misma secuencia con distinto canal producían informes
    /// idénticos, porque la condición en la que se midió no viajaba a ningún
    /// sitio (Regla 3 de ADR-0019, por la puerta que aquel ADR no miró).
    pub parametros: Vec<(String, expr::Value)>,
    /// Los valores con nombre que **devolvió** el paso además de la medida.
    /// Vienen del cable (tag 7). No participan en el veredicto: `asigna` los
    /// lee como `resultado.salidas.<nombre>` y los sinks los escriben.
    pub salidas: Vec<(String, expr::Value)>,
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
            fase: Fase::Main,
            parametros: Vec::new(),
            salidas: Vec::new(),
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
        self.estado == "pass"
    }
}

/// El resultado agregado de una secuencia corrida.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultadoSecuencia {
    pub nombre: String,
    pub pasos: Vec<ResultadoStep>,
    /// La secuencia declaraba un veredicto (al menos un paso `pass_fail` en
    /// `main`) y **ninguno llegó a evaluarse** (ADR-0019, Regla 1, issue #31).
    ///
    /// Lo sella **el motor**, que es el único que lo sabe: un `ResultadoStep`
    /// no lleva el tipo del paso que lo produjo, así que `estado()` no puede
    /// deducirlo de `pasos`. Un consumidor que construya un `ResultadoSecuencia`
    /// a mano (tests, sinks) lo deja en `false`, que es el `Default`.
    pub veredicto_sin_evaluar: bool,
}

impl ResultadoSecuencia {
    pub fn nueva(nombre: &str) -> Self {
        ResultadoSecuencia {
            nombre: nombre.to_string(),
            pasos: Vec::new(),
            veredicto_sin_evaluar: false,
        }
    }

    /// Añade un resultado de paso al agregado de la secuencia.
    pub fn registra(&mut self, paso: ResultadoStep) {
        self.pasos.push(paso);
    }

    /// Cuántos pasos se saltaron y cuántos hay en total, contando el árbol
    /// entero (los `sub_pasos` de un sequence call incluidos).
    ///
    /// `saltado` es **neutral** en el agregado por diseño (RF-33/34: un paso
    /// saltado por `disable` o por precondición falsa no es un fallo), pero
    /// esa neutralidad esconde cuánto dejó de correrse: en la primera campaña
    /// de beta, 9 secuencias daban verde saltándose ≥30% de sus pasos y no se
    /// vio hasta auditar los ficheros a mano. El ratio necesita los dos
    /// números, así que se devuelven juntos.
    pub fn saltados(&self) -> (usize, usize) {
        fn cuenta(pasos: &[ResultadoStep], saltados: &mut usize, total: &mut usize) {
            for p in pasos {
                *total += 1;
                if p.estado == "skipped" {
                    *saltados += 1;
                }
                if let Some(sub) = &p.sub_pasos {
                    cuenta(sub, saltados, total);
                }
            }
        }
        let (mut saltados, mut total) = (0, 0);
        cuenta(&self.pasos, &mut saltados, &mut total);
        (saltados, total)
    }

    /// Estado agregado de la secuencia: **el más severo de sus pasos**
    /// (ADR-0019, Regla 1), en la escala `paso < inconcluso < fallo < error`.
    /// `saltado` es neutral y no la mueve.
    ///
    /// Antes esto era una cascada `error > fallo > paso` cuyo `else` devolvía
    /// `paso`. Ese `else` era el issue #31: una secuencia cuyo veredicto no se
    /// llegó a evaluar no había fallado, luego «pasaba». `paso` es una
    /// afirmación sobre lo comprobado, no lo que queda cuando no hay nada malo
    /// que decir; de ahí `veredicto_sin_evaluar`, que eleva el agregado a
    /// `inconcluso` sin tapar un `fallo` ni un `error` que también estén.
    ///
    /// No desciende a `sub_pasos`: el `ResultadoStep` de un `sequence_call` ya
    /// trae el agregado de su subsecuencia, calculado aquí mismo, así que la
    /// severidad de un descendiente profundo llega a la raíz nivel a nivel.
    pub fn estado(&self) -> &'static str {
        let peor = self
            .pasos
            .iter()
            .map(|p| Severidad::de(&p.estado))
            .max()
            .unwrap_or_default();
        let peor = if self.veredicto_sin_evaluar {
            peor.max(Severidad::Inconcluso)
        } else {
            peor
        };
        peor.como_texto()
    }

    /// Reporte de la secuencia en texto. El formato es parte de la spec
    /// (RNF-08): no se toca sin querer tocar la especificación.
    ///
    /// Escribe al `Write` que se le pase, para que el sink de consola (y
    /// los tests) no se acoplen a stdout. `reporte()` (la API pública
    /// congelada) delega aquí con `stdout` y produce los mismos bytes que
    /// el `println!` original.
    /// Extensión aditiva de RNF-08 (como el `"skipped"` de M4 y el anidamiento
    /// de M4b): si algún paso se saltó, se cierra con una línea de recuento.
    /// Una corrida sin saltos produce exactamente los bytes de siempre, y las
    /// líneas de paso no cambian; lo que se añade es una línea que antes no
    /// existía, para que un verde con la mitad de la secuencia sin correr no
    /// pase inadvertido en consola.
    pub fn reporte_a(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(w, "=== {}: {} ===", self.nombre, self.estado())?;
        for p in &self.pasos {
            Self::escribe_paso(w, p, 1)?;
        }
        let (saltados, total) = self.saltados();
        if saltados > 0 {
            writeln!(w, "  ({saltados} de {total} pasos saltados)")?;
        }
        Ok(())
    }

    /// Escribe un `ResultadoStep` (y, recursivamente, sus `sub_pasos`) al
    /// reporte textual. `nivel` es la profundidad de indentación (1 para
    /// los pasos top-level = 2 espacios, como el formato congelado de M0;
    /// 2+ para sub-pasos de un sequence call). Extensión aditiva de RNF-08:
    /// un paso sin `sub_pasos` produce exactamente la misma línea que antes.
    fn escribe_paso(
        w: &mut impl std::io::Write,
        p: &ResultadoStep,
        nivel: usize,
    ) -> std::io::Result<()> {
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
/// `PassFail` (ADR-0018) evalúa una expresión booleana y produce el veredicto.
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
    /// Veredicto por expresión (RF-25, ADR-0018): el motor evalúa `condicion`
    /// contra su entorno y produce `paso`/`fallo` — el criterio de aceptación
    /// **compuesto** sobre medidas ya volcadas a variables. Local, sin gRPC.
    /// Es el análogo del step type `Pass/Fail Test` de TestStand, cuyo data
    /// source es una expresión booleana.
    PassFail,
}

/// Cómo se invoca un ejecutor de pasos (M5-ext.1, RF-36.3). El motor
/// despacha por el nombre de ejecutor declarado en `DefinicionPaso.ejecutor`;
/// sin declaración, va al embebido (default). El motor no sabe qué hay
/// detrás de cada endpoint: WASM embebido, `.wasm` cargado por el host
/// (M5-ext.2, ADR-0013) o un gRPC remoto (ADR-0013).
#[derive(Debug, Clone, PartialEq)]
pub enum TipoEjecutor {
    /// El ejecutor WASM de serie, compilado dentro del host (ADR-0011).
    /// Endpoint fijo `127.0.0.1:9100`. Default si el paso no declara
    /// `ejecutor`.
    Embebido,
    /// Módulo `.wasm` propio que el **host** carga por path en runtime
    /// (RF-36.2, M5-ext.2; el ejecutor embebido no puede, ADR-0013). Es una
    /// **directiva de carga para el host** (ADR-0014): el cargador la valida
    /// al cargar (el path debe existir), el host la instancia y la expone
    /// como `grpc` (override `--executor`); el motor **nunca la ejecuta**
    /// (si llega sin traducir, `Error::EjecutorWasmSinHost`).
    Wasm { path: String },
    /// Ejecutor de lenguaje distribuido (Python, …) accesible por gRPC
    /// (RF-36.1). `host:puerto` puede ser no-loopback **sólo si se declara**
    /// en el YAML (relajación acotada del loopback de ADR-0011).
    Grpc { host: String, puerto: u16 },
}

/// Un ejecutor declarado en `ejecutores:` del YAML (M5-ext.1, RF-36.3). El
/// cargador lo traduce y lo registra en `Programa.ejecutores`; el motor lo
/// consulta para resolver el endpoint de un paso.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinicionEjecutor {
    /// Nombre declarado en el YAML; los pasos lo referencian con
    /// `ejecutor: <nombre>`. El nombre interno `__anvil_embebido__` está
    /// reservado (lo usa el motor; el cargador lo rechaza).
    pub nombre: String,
    pub tipo: TipoEjecutor,
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

/// Un parámetro de entrada de un paso `Grpc`, tal y como queda tras cargar
/// (ADR-0020 §2).
///
/// O es un literal —y su tipo es el del escalar YAML— o es una expresión
/// `${...}` que el motor evalúa **antes** de llamar, contra su propio entorno
/// (ADR-0009: las expresiones las evalúa el motor; el paso no ve `locals`,
/// se le pasa un valor).
///
/// Una expresión que falla convierte el paso en `error`, **nunca en un valor
/// por defecto**: un banco que mide con un parámetro inventado da un número
/// que parece bueno y no lo es.
#[derive(Debug, Clone, PartialEq)]
pub enum EntradaPaso {
    Literal(ValorDefinicion),
    Expresion(expr::Expresion),
}

/// The engine's internal key for the connection to the built-in WASM executor
/// (ADR-0011).
///
/// It cannot be declared in a YAML — the loader rejects an executor with this
/// name — and it lives here, in the lowest crate, because three of them need
/// the same string and must not disagree: the loader (to reject it and to
/// resolve routing), the engine (to key its connections) and the report sinks
/// (to render a reference's executor without printing plumbing).
pub const EJECUTOR_EMBEBIDO: &str = "__anvil_embebido__";

/// How an executor's routing key is written for a human: the reserved key of
/// the built-in one is plumbing and is shown by the name the docs use.
pub fn nombre_visible_de_ejecutor(clave: &str) -> &str {
    if clave == EJECUTOR_EMBEBIDO {
        "embebido"
    } else {
        clave
    }
}

/// El valor literal de una variable declarada en el YAML (scopes
/// `locals`/`parameters`/`file_globals`). El tipo se infiere del escalar YAML:
/// número, texto o booleano. Sin árbol de propiedades tipado recursivo de
/// TestStand en el MVP (ver [variables-y-alcances.md](../../docs/diseno/variables-y-alcances.md)).
///
/// [`ValorDefinicion::Reference`] is the odd one out and deliberately so: it
/// is a **declaration without a value**. A reference cannot be written by hand
/// —that is one of the four refusals ADR-0022 §1 buys— so the only thing a
/// sequence can say about a `locals:` entry that will hold a handle is what
/// executor it will come from.
#[derive(Debug, Clone, PartialEq)]
pub enum ValorDefinicion {
    Numero(f64),
    Texto(String),
    Bool(bool),
    /// `locals: { rack: { type: reference, executor: bench } }` (ADR-0022 §3).
    ///
    /// The executor is part of the declaration and not decoration: it is the
    /// **only** thing that makes the cross-executor hand-off decidable without
    /// data-flow analysis. `inputs: { rack: '${locals.rack}' }` is an
    /// expression, and the type of an expression is not guessed (ADR-0021 §5);
    /// following the handle back to the step that minted it would mean walking
    /// `assign`, subsequence `args` and the process model, which is exactly the
    /// analysis ADR-0021 declined. Declared on the variable, the check is one
    /// lookup and it can be seen by reading the file.
    Reference {
        executor: String,
    },
}

impl ValorDefinicion {
    /// Traduce un literal declarado en el YAML al `Value` del expression engine
    /// (`expr::Value`). Lo usa el motor al materializar el entorno al inicio
    /// de la secuencia.
    ///
    /// A [`ValorDefinicion::Reference`] materialises as `Nulo`: it declares
    /// that the variable **will** hold a handle, and until a step mints one
    /// there is nothing there. Reading it before then and handing it to a step
    /// is refused where every other absent parameter is (`evalua_entradas`),
    /// which is the same rule and not a new one.
    pub fn a_value(&self) -> expr::Value {
        match self {
            ValorDefinicion::Numero(x) => expr::Value::Numero(*x),
            ValorDefinicion::Texto(s) => expr::Value::Texto(s.clone()),
            ValorDefinicion::Bool(b) => expr::Value::Bool(*b),
            ValorDefinicion::Reference { .. } => expr::Value::Nulo,
        }
    }

    /// The executor a reference declaration is bound to, or `None` for the
    /// three scalars.
    pub fn ejecutor_de_referencia(&self) -> Option<&str> {
        match self {
            ValorDefinicion::Reference { executor } => Some(executor),
            _ => None,
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
    /// RF-34: si `true`, el motor registra el paso como saltado (`"skipped"`)
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
    /// RF-25 (ADR-0018): expresión booleana del veredicto si
    /// `tipo == PassFail`. La evalúa el **motor** contra su entorno —el paso
    /// no interviene, igual que con `limite` (ADR-0008) y `precondicion`
    /// (ADR-0009)—: `true` → `paso`, `false` → `fallo`, no-Bool → `error`.
    /// AST parseado por el cargador al cargar (fail-fast). `None` salvo en
    /// `PassFail`; el cargador valida la coherencia con `tipo`.
    pub condicion: Option<expr::Expresion>,
    /// M4b/RF-27: destino de la subsecuencia si `tipo == SequenceCall`. Es
    /// un **nombre** (subsecuencia inline del mismo archivo) o un **path
    /// relativo** (archivo externo); el cargador distingue por la
    /// convención `es_path` (ver `formato-de-secuencia.md`). `None` si no
    /// es `SequenceCall`.
    pub secuencia: Option<String>,
    /// M4b/RF-27: argumentos by-reference del sequence call (`locals.X`
    /// ↔ `parameters.param`). `None` si no es `SequenceCall`.
    pub parametros: Option<Vec<Argumento>>,
    /// ADR-0020: los parámetros **de entrada** de un paso `Grpc`, que viajan
    /// by-value en la petición. `None` = el paso no declara ninguno, y
    /// entonces un ejecutor de contrato 1 sigue siendo válido.
    ///
    /// **Se llama `entradas` y no `parametros` porque ese nombre ya está
    /// cogido** por los argumentos by-reference del `sequence_call` (arriba),
    /// que son otra cosa: aquéllos son lvalues que se escriben de vuelta,
    /// éstos son valores que se envían. En el YAML los dos se declaran como
    /// `parametros:` —son mutuamente excluyentes por `tipo`— y el cargador
    /// rechaza el caso en que confundirlos cambiaría el significado en
    /// silencio (ver `EntradaPaso`).
    ///
    /// Ordenado por nombre al cargar: el orden del cable tiene que ser
    /// determinista para que dos corridas iguales produzcan bytes iguales.
    pub entradas: Option<Vec<(String, EntradaPaso)>>,
    /// M5-ext.1 (RF-36.3): nombre del ejecutor que atiende este paso. Si es
    /// `None`, el motor usa el ejecutor **embebido** (default,
    /// `127.0.0.1:9100`) — compat con M4b. El cargador valida que el nombre
    /// exista en `Programa.ejecutores` (fail-fast al cargar).
    pub ejecutor: Option<String>,
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
            condicion: None,
            secuencia: None,
            parametros: None,
            entradas: None,
            ejecutor: None,
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
    /// M5-ext.1 (RF-36.3): ejecutores declarados en `ejecutores:` del YAML
    /// de la secuencia raíz, keyed por nombre. El motor los consulta para
    /// despachar por `DefinicionPaso.ejecutor`. Sin entradas, todo va al
    /// ejecutor embebido (compat con M4b, ADR-0011).
    pub ejecutores: HashMap<String, DefinicionEjecutor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_agregado() {
        let mut s = ResultadoSecuencia::nueva("s");
        assert_eq!(s.estado(), "pass", "una secuencia vacía pasa");

        s.registra(ResultadoStep::nuevo("a", "pass", "ok"));
        assert_eq!(s.estado(), "pass");

        s.registra(ResultadoStep::nuevo("b", "fail", "mal"));
        assert_eq!(s.estado(), "fail");

        // error manda sobre fallo, aunque llegue después.
        s.registra(ResultadoStep::nuevo("c", "error", "peor"));
        assert_eq!(s.estado(), "error");
    }

    #[test]
    fn error_manda_aunque_llegue_antes() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "error", "peor"));
        s.registra(ResultadoStep::nuevo("b", "fail", "mal"));
        assert_eq!(s.estado(), "error");
    }

    /// ADR-0019, Regla 1: la escala es el orden de declaración del enum, como
    /// el entero del `Verdict` de OpenTAP. Si alguien reordena las variantes,
    /// la agregación cambia de significado en silencio; esto lo impide.
    #[test]
    fn la_severidad_esta_ordenada() {
        assert!(Severidad::Paso < Severidad::Inconcluso);
        assert!(Severidad::Inconcluso < Severidad::Fallo);
        assert!(Severidad::Fallo < Severidad::Error);
    }

    /// `saltado` es neutral (RF-33/34) y no entra en la escala.
    #[test]
    fn saltado_es_neutral_en_la_escala() {
        assert_eq!(Severidad::de("skipped"), Severidad::Paso);
        assert_eq!(Severidad::de("pass"), Severidad::Paso);
    }

    /// ADR-0019, Regla 2 (issue #28): un estado que nadie reconoce es `Error`,
    /// **no** neutral. Esta severidad ya se movió una vez sin querer —al
    /// introducir la escala, la rama `_` de `Severidad::de` mandaba lo
    /// desconocido a `Paso`, que convirtió un `fallo` mudo en un verde mudo—,
    /// así que aquí queda fijada: si alguien la cambia, que sea a sabiendas.
    #[test]
    fn un_estado_no_reconocido_es_error() {
        assert_eq!(Severidad::de("Paso"), Severidad::Error, "#28");
        assert_eq!(Severidad::de("PASS"), Severidad::Error);
        assert_eq!(Severidad::de(""), Severidad::Error);
        assert_eq!(Severidad::de("cualquier cosa"), Severidad::Error);
        // Un ejecutor tampoco puede declararse a sí mismo no concluyente: eso
        // lo produce el motor al agregar (ADR-0019, «Recortes»). Como cadena
        // suelta sí es un agregado legítimo, y por eso no cae en la rama `_`.
        assert_eq!(Severidad::de("inconclusive"), Severidad::Inconcluso);
    }

    /// Una secuencia no se pone verde porque un ejecutor escribiera mal el
    /// estado. Es el issue #28 visto desde el agregado, que es donde dolía.
    #[test]
    fn un_estado_no_reconocido_no_deja_pasar_la_secuencia() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo(
            "verificar_led",
            "Paso",
            "led encendido",
        ));
        assert_eq!(s.estado(), "error");
    }

    /// Los dos vocabularios cerrados de la Regla 2, fijados: cada estado que un
    /// ejecutor puede devolver tiene rama propia en la escala.
    #[test]
    fn los_estados_de_ejecutor_estan_todos_en_la_escala() {
        for e in ESTADOS_DE_EJECUTOR {
            let sev = Severidad::de(e);
            if e == "error" {
                assert_eq!(sev, Severidad::Error);
            } else {
                assert_ne!(sev, Severidad::Error, "'{e}' es un estado válido");
            }
        }
        assert_eq!(CAMPOS_RESULTADO.len(), 3, "los campos son tres y cerrados");
    }

    /// El issue #31: la secuencia declaraba un veredicto y no llegó a
    /// evaluarse. No hay ningún paso en rojo —el `pass_fail` está `saltado`—
    /// y aun así la secuencia no puede decir `paso`.
    #[test]
    fn un_veredicto_sin_evaluar_deja_la_secuencia_inconclusa() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("init", "pass", "ok"));
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        assert_eq!(s.estado(), "pass", "sin el sello del motor, nada cambia");

        s.veredicto_sin_evaluar = true;
        assert_eq!(
            s.estado(),
            "inconclusive",
            "una unidad que nadie midió no puede salir aprobada"
        );
    }

    /// `inconcluso` **eleva** la severidad, nunca la baja. Es la propiedad del
    /// `UpgradeVerdict` de OpenTAP: si además hay un fallo o un error, esos
    /// mandan — perder un `fallo` detrás de un `inconcluso` sería reintroducir
    /// el verde falso por el otro lado.
    #[test]
    fn inconcluso_no_tapa_un_fallo() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "fail", "fuera de rango"));
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        s.veredicto_sin_evaluar = true;
        assert_eq!(s.estado(), "fail");
    }

    #[test]
    fn inconcluso_no_tapa_un_error() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "error", "el banco no responde"));
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        s.veredicto_sin_evaluar = true;
        assert_eq!(s.estado(), "error");
    }

    /// Un `sequence_call` cuya subsecuencia quedó inconclusa trae el estado ya
    /// agregado en su propio `ResultadoStep`; el padre lo recoge por la escala,
    /// igual que un `fallo`. Es la propagación nivel a nivel que el ADR pide no
    /// dejar divergir entre los dos caminos de agregación.
    #[test]
    fn el_inconcluso_de_una_subsecuencia_sube_al_padre() {
        let mut s = ResultadoSecuencia::nueva("padre");
        s.registra(ResultadoStep::nuevo("a", "pass", "ok"));
        let mut call = ResultadoStep::nuevo("sub", "inconclusive", "sequence call → inconcluso");
        call.sub_pasos = Some(vec![ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        )]);
        s.registra(call);
        assert_eq!(s.estado(), "inconclusive");
    }

    /// RNF-08 es «el formato textual no se cambia sin querer», no «no se añaden
    /// estados» (M4 ya añadió `saltado` como extensión aditiva). Lo único que
    /// cambia en el texto es la cabecera; el paso se sigue reportando
    /// `[skipped]`, que es lo que ocurrió.
    #[test]
    fn el_reporte_de_una_secuencia_inconclusa() {
        let mut s = ResultadoSecuencia::nueva("b31");
        s.registra(ResultadoStep::nuevo("init", "pass", "statement ok"));
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        s.veredicto_sin_evaluar = true;

        let mut buf = Vec::new();
        s.reporte_a(&mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "=== b31: inconclusive ===\n  \
             [pass] init: statement ok\n  \
             [skipped] verdict: precondición falsa\n  \
             (1 de 2 pasos saltados)\n"
        );
    }

    /// RNF-08: el formato textual de `reporte_a` es spec congelada.
    /// Este test congela los bytes exactos para detectar cambios
    /// accidentales.
    #[test]
    fn reporte_a_congela_el_formato() {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::medido(
            "medir_voltaje",
            "fail",
            "voltaje fuera de rango",
            4.2,
            4.5,
            5.5,
        ));
        s.registra(ResultadoStep::nuevo(
            "verificar_led",
            "pass",
            "led encendido",
        ));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== basica: fail ===
  [fail] medir_voltaje: voltaje fuera de rango
  [pass] verificar_led: led encendido
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }

    #[test]
    fn limite_rango_dentro_fuera_y_fronteras() {
        let r = Limite::Rango { min: 4.5, max: 5.5 };
        assert_eq!(r.evalua(5.0), "pass", "dentro del rango");
        assert_eq!(r.evalua(4.2), "fail", "por debajo");
        assert_eq!(r.evalua(6.0), "fail", "por encima");
        // Fronteras inclusivas.
        assert_eq!(r.evalua(4.5), "pass", "min incluido");
        assert_eq!(r.evalua(5.5), "pass", "max incluido");
    }

    #[test]
    fn limite_comparacion_cubre_seis_operadores() {
        use Operador::*;
        assert_eq!(
            Limite::Comparacion {
                op: Eq,
                esperado: 1000.0
            }
            .evalua(1000.0),
            "pass"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Eq,
                esperado: 1000.0
            }
            .evalua(999.0),
            "fail"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Ne,
                esperado: 1000.0
            }
            .evalua(999.0),
            "pass"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Lt,
                esperado: 1000.0
            }
            .evalua(999.0),
            "pass"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Lt,
                esperado: 1000.0
            }
            .evalua(1000.0),
            "fail",
            "lt excluye el igual"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Le,
                esperado: 1000.0
            }
            .evalua(1000.0),
            "pass",
            "le incluye el igual"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Gt,
                esperado: 1000.0
            }
            .evalua(1001.0),
            "pass"
        );
        assert_eq!(
            Limite::Comparacion {
                op: Ge,
                esperado: 1000.0
            }
            .evalua(1000.0),
            "pass"
        );
    }

    #[test]
    fn operador_simbolo_y_parseo_ida_y_vuelta() {
        for op in [
            Operador::Eq,
            Operador::Ne,
            Operador::Lt,
            Operador::Le,
            Operador::Gt,
            Operador::Ge,
        ] {
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
        assert_eq!(
            Operador::de_texto("  eq  "),
            Some(Operador::Eq),
            "tolera espacios"
        );
        // Símbolos conocidos para el reporte.
        assert_eq!(Operador::Ge.simbolo(), ">=");
        assert_eq!(Operador::Ne.simbolo(), "!=");
    }

    #[test]
    fn definicion_paso_con_limite_lo_guarda() {
        let p =
            DefinicionPaso::con_limite("medir_voltaje", 1, Limite::Rango { min: 4.5, max: 5.5 });
        assert_eq!(p.limite, Some(Limite::Rango { min: 4.5, max: 5.5 }));
        // Sin límite por defecto: el paso decide (pass/fail, action).
        assert_eq!(DefinicionPaso::nuevo("verificar_led", 1).limite, None);
    }

    #[test]
    fn medido_valor_deja_limites_vacios() {
        // Un paso que mide sin conocer el umbral: el motor rellena los
        // campos de límite después, desde el YAML (ADR-0008).
        let r = ResultadoStep::medido_valor("medir_voltaje", "pass", "medido: 4.2 V", 4.2);
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
        s.registra(ResultadoStep::nuevo("a", "skipped", "disable"));
        assert_eq!(s.estado(), "pass");
        // Un saltado junto a un fallo: manda el fallo, no se anula.
        s.registra(ResultadoStep::nuevo("b", "fail", "mal"));
        assert_eq!(s.estado(), "fail");
    }

    /// RNF-08 (extensión aditiva de M4): el estado `"skipped"` aparece en el
    /// reporte textual congelado. El formato de línea no cambia; sólo se
    /// añade un nuevo *valor* de estado.
    #[test]
    fn reporte_incluye_estado_saltado() {
        let mut s = ResultadoSecuencia::nueva("variables");
        s.registra(ResultadoStep::nuevo("init_log", "pass", "statement ok"));
        s.registra(ResultadoStep::nuevo("paso_obsoleto", "skipped", "disable"));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        // La línea de recuento la añade #13: un verde que se salta la mitad
        // de la secuencia tiene que decirlo en consola.
        let esperado = "\
=== variables: pass ===
  [pass] init_log: statement ok
  [skipped] paso_obsoleto: disable
  (1 de 2 pasos saltados)
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }

    /// Sin saltos, el reporte produce exactamente los bytes de siempre: la
    /// línea de recuento no aparece (RNF-08, extensión aditiva).
    #[test]
    fn reporte_sin_saltados_no_lleva_linea_de_recuento() {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::nuevo("medir", "pass", "ok"));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "=== basica: pass ===\n  [pass] medir: ok\n"
        );
    }

    #[test]
    fn saltados_cuenta_el_arbol_entero() {
        // Un sequence call cuyos hijos se saltan: lo que importa al triar es
        // cuántos pasos no corrieron, en cualquier nivel.
        let mut call = ResultadoStep::nuevo("test_uut", "pass", "sequence call → paso");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_1", "skipped", "precondición falsa"),
            ResultadoStep::nuevo("medir_2", "pass", "ok"),
        ]);
        let mut s = ResultadoSecuencia::nueva("raiz");
        s.registra(ResultadoStep::nuevo("preparar", "skipped", "disable"));
        s.registra(call);

        // 4 pasos en el árbol (preparar, el call y sus dos hijos), 2 saltados.
        assert_eq!(s.saltados(), (2, 4));
        // Y el agregado sigue siendo `paso`: la neutralidad no cambia (RF-33/34).
        assert_eq!(s.estado(), "pass");
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
        let r = ResultadoStep::nuevo("p", "pass", "ok");
        assert_eq!(r.sub_pasos, None);
        let m = ResultadoStep::medido_valor("m", "pass", "ok", 4.2);
        assert_eq!(m.sub_pasos, None);
    }

    /// M4b: el estado agregado de un sequence call es el de la subsecuencia
    /// (se anida en `sub_pasos`); el agregado de la secuencia padre mira
    /// `p.estado` del call (que ya es el agregado), sin descender.
    #[test]
    fn estado_agregado_con_sequence_call_anidado() {
        let mut call = ResultadoStep::nuevo("test_fuentes", "fail", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "pass", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fail", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        // El call ya trae "fail" (agregado de la sub); la secuencia padre
        // lo ve sin descender a sub_pasos.
        assert_eq!(s.estado(), "fail");
    }

    /// RNF-08 (extensión aditiva de M4b): el reporte textual anida los
    /// sub-pasos de un sequence call con +2 espacios por nivel. Los pasos
    /// sin `sub_pasos` siguen produciendo la misma línea de siempre.
    #[test]
    fn reporte_anida_sub_pasos() {
        let mut call = ResultadoStep::nuevo(
            "test_fuentes",
            "fail",
            "sequence call './medir_fuentes.yaml' → fallo",
        );
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "pass", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fail", "fuera de rango"),
            ResultadoStep::nuevo("desconectar", "pass", "ok"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== basica: fail ===
  [fail] test_fuentes: sequence call './medir_fuentes.yaml' → fallo
    [pass] medir_canal_1: ok
    [fail] medir_canal_2: fuera de rango
    [pass] desconectar: ok
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
        // M5-ext.1: `ejecutores` también vacío por defecto.
        assert!(p.ejecutores.is_empty());
    }

    /// M5-ext.1: un paso nuevo no declara ejecutor → embebido (compat M4b),
    /// y el `Programa` vacío no trae ejecutores.
    #[test]
    fn paso_nuevo_sin_ejecutor_y_programa_sin_ejecutores() {
        let p = DefinicionPaso::nuevo("verificar_led", 1);
        assert_eq!(p.ejecutor, None);
        assert_eq!(
            DefinicionPaso::con_limite("m", 1, Limite::Rango { min: 1.0, max: 2.0 }).ejecutor,
            None
        );
    }

    /// M5-ext.1: los tres variantes de `TipoEjecutor` se construyen.
    #[test]
    fn tipo_ejecutor_tres_variantes() {
        assert_eq!(TipoEjecutor::Embebido, TipoEjecutor::Embebido);
        assert_eq!(
            TipoEjecutor::Wasm {
                path: "./p.wasm".into()
            },
            TipoEjecutor::Wasm {
                path: "./p.wasm".into()
            }
        );
        assert_eq!(
            TipoEjecutor::Grpc {
                host: "127.0.0.1".into(),
                puerto: 9101
            },
            TipoEjecutor::Grpc {
                host: "127.0.0.1".into(),
                puerto: 9101
            }
        );
    }
}
