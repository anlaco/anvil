//! Los valores del lenguaje de expresiones de anvil.
//!
//! Un subconjunto acotado (RF-35): número, booleano, texto, **referencia** y
//! `Nulo`. No hay listas ni records en el MVP (post-MVP). El motor de test
//! manipula estos valores por scopes (ver [`crate::eval::Entorno`]); el engine
//! **no** conoce `ResultadoStep` ni el dominio (ADR-0005).

/// A handle to an object an executor keeps for itself (ADR-0022).
///
/// **It names a slot, not an object.** Mutating the state behind it does not
/// change its identity: a step that reconfigures the bench answers with the
/// very reference it was given. A step mints a new one only when another
/// object was really born — deriving one configuration from another,
/// duplicating (ADR-0022 §5).
///
/// The three parts are minted by different parties and neither claims anything
/// about the other's (ADR-0022 §4):
///
/// - `executor`: stamped by **Anvil**, on receiving the reference. The process
///   on the far side does not know what the sequence called it — the names
///   live in the YAML's `executors:`, which is also what the engine routes on.
/// - `lifetime`: minted by the **executor** when it starts, and published in
///   its `Catalog`. It is what makes a restart detectable: a type system
///   cannot say whether the process opposite died and was born again.
/// - `payload`: minted by the **executor**, and **opaque to Anvil**, which
///   never interprets it, never composes it and never writes one by hand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    /// The name the sequence gives the executor that keeps the object.
    pub executor: String,
    /// The executor's life this reference was born under. Empty means the
    /// executor does not publish one, and liveness therefore cannot be
    /// checked — which is said out loud, never assumed (ADR-0019, Rule 2).
    pub lifetime: String,
    /// What the executor uses to find the object again. Anvil never reads it.
    pub payload: String,
}

impl Reference {
    /// A reference as it is written in a report or an error message:
    /// `<executor>/<lifetime>/<payload>`, no quoting.
    ///
    /// It is for a human, not for parsing: the report sinks have their own,
    /// unambiguous forms (`json::valor_a_json`, `csv::referencia_a_token`).
    pub fn mostrar(&self) -> String {
        format!("{}/{}/{}", self.executor, self.lifetime, self.payload)
    }
}

/// Un valor del lenguaje. `Nulo` representa la ausencia — p. ej.
/// `resultado.valor_medido` cuando el paso no midió nada.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Número en coma flotante (el proyecto usa `f64` en `Limite`/`valor_medido`).
    Numero(f64),
    /// Booleano. Resultado de comparaciones y de `and`/`or`/`not`.
    Bool(bool),
    /// Cadena de texto. P. ej. `resultado.estado` o un `file_globals` textual.
    Texto(String),
    /// A handle to an object the executor keeps (ADR-0022). **Opaque**: the
    /// engine can carry it from an `assign` to an `inputs:` and can refuse
    /// every other operation on it, and that is the whole of what it can do.
    Reference(Reference),
    /// Ausencia. Análogo al `None` de `Option<f64>` en `resultado.valor_medido`.
    Nulo,
}

impl Value {
    /// `true` si es `Numero`. Usado por el evaluator para validar operandos.
    pub fn es_numero(&self) -> bool {
        matches!(self, Value::Numero(_))
    }

    /// `true` si es `Bool`.
    pub fn es_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    /// The reference this value holds, or `None`. Used where the engine has to
    /// **refuse** — an operation, a cross-executor hand-off — rather than
    /// where it has to compute.
    pub fn reference(&self) -> Option<&Reference> {
        match self {
            Value::Reference(r) => Some(r),
            _ => None,
        }
    }

    /// Descripción corta para mensajes de error de tipo
    /// (p. ej. `"número"`, `"bool"`, `"texto"`, `"nulo"`).
    pub fn tipo(&self) -> &'static str {
        match self {
            Value::Numero(_) => "número",
            Value::Bool(_) => "bool",
            Value::Texto(_) => "texto",
            Value::Reference(_) => "referencia",
            Value::Nulo => "nulo",
        }
    }

    /// Como `Display`, pero sin formato JSON ni comillas: el texto se muestra
    /// tal cual, los números sin ceros sobrantes. Lo usa el motor para el
    /// mensaje de un paso `statement`.
    pub fn mostrar(&self) -> String {
        match self {
            Value::Numero(x) => formato_numero(*x),
            Value::Bool(b) => b.to_string(),
            Value::Texto(s) => s.clone(),
            Value::Reference(r) => r.mostrar(),
            Value::Nulo => "nulo".to_string(),
        }
    }
}

impl std::fmt::Display for Value {
    /// Forma canónica legible (con comillas para texto, para distinguirlo de
    /// un identificador en mensajes de error).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Numero(x) => write!(f, "{}", formato_numero(*x)),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Texto(s) => write!(f, "\"{s}\""),
            // Angle brackets and not quotes: a reference is not a text and
            // showing it as one is exactly the confusion ADR-0022 §1 exists to
            // stop.
            Value::Reference(r) => write!(f, "<referencia {}>", r.mostrar()),
            Value::Nulo => write!(f, "nulo"),
        }
    }
}

/// Formatea un `f64` sin colas `.0` en enteros ni ceros depreciables, para que
/// los mensajes del motor sean limpios (`4.2`, no `4.200000000001`; `5`, no
/// `5.0`). Sigue la misma convención que `modelo::proto::a_texto`.
fn formato_numero(x: f64) -> String {
    if x.fract() == 0.0 && x.is_finite() {
        format!("{}", x as i64)
    } else {
        // `{}` sobre f64 ya imprime la forma más corta que recupera el valor
        // (Rust usa Grisu); es suficiente y determinista para el MVP.
        format!("{x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tipo_de_cada_variante() {
        assert_eq!(Value::Numero(1.0).tipo(), "número");
        assert_eq!(Value::Bool(true).tipo(), "bool");
        assert_eq!(Value::Texto("x".into()).tipo(), "texto");
        assert_eq!(referencia().tipo(), "referencia");
        assert_eq!(Value::Nulo.tipo(), "nulo");
    }

    #[test]
    fn mostrar_entero_sin_cola() {
        assert_eq!(Value::Numero(5.0).mostrar(), "5");
        assert_eq!(Value::Numero(4.2).mostrar(), "4.2");
        assert_eq!(Value::Bool(false).mostrar(), "false");
        assert_eq!(Value::Texto("A-2026".into()).mostrar(), "A-2026");
        assert_eq!(Value::Nulo.mostrar(), "nulo");
    }

    fn referencia() -> Value {
        Value::Reference(Reference {
            executor: "python".into(),
            lifetime: "l1".into(),
            payload: "rack-1".into(),
        })
    }

    /// A reference does not look like a text anywhere it is shown. The two
    /// forms differ on purpose: `Display` is for diagnostics and says what it
    /// is; `mostrar` is the bare identity, for a report line.
    #[test]
    fn a_reference_is_never_shown_as_a_text() {
        assert_eq!(referencia().mostrar(), "python/l1/rack-1");
        assert_eq!(referencia().to_string(), "<referencia python/l1/rack-1>");
        assert_ne!(
            referencia().to_string(),
            Value::Texto("python/l1/rack-1".into()).to_string()
        );
    }
}
