//! Los valores del lenguaje de expresiones de anvil.
//!
//! Un subconjunto acotado (RF-35): número, booleano, texto y `Nulo`. No hay
//! listas ni records en el MVP (post-MVP). El motor de test manipula estos
//! valores por scopes (ver [`crate::eval::Entorno`]); el engine **no** conoce
//! `ResultadoStep` ni el dominio (ADR-0005).

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

    /// Descripción corta para mensajes de error de tipo
    /// (p. ej. `"número"`, `"bool"`, `"texto"`, `"nulo"`).
    pub fn tipo(&self) -> &'static str {
        match self {
            Value::Numero(_) => "número",
            Value::Bool(_) => "bool",
            Value::Texto(_) => "texto",
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
}