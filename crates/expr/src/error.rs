//! Errores del motor de expresiones.
//!
//! El engine **nunca pánics**: toda API pública devuelve [`Result<_, ErrorExpr>`].
//! Esto es no negociable: el engine corre dentro del motor, que no puede caer
//! por una expresión mal escrita. El motor traduce un `ErrorExpr` en un
//! `ResultadoStep` con `estado = "error"` (ver `crates/motor/src/lib.rs`).

/// Clase de error. Determina de qué fase del engine viene y ayuda a los
/// mensajes y a los tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// No se pudo parsear (token inesperado, EOF prematuro, `=` en posición
    /// de expresión, comparaciones encadenadas tipo `a < b < c`, …).
    Sintaxis,
    /// Token no reconocido por el lexer (carácter raro, string sin cerrar).
    Lexico,
    /// Operandos incompatibles para un operador (p. ej. `Numero and Bool`).
    /// Se detecta en runtime, al evaluar.
    Tipo,
    /// Error de evaluación no atribuible a tipos: división por cero, `Nulo`
    /// en aritmética/comparación de orden, etc.
    Evaluacion,
    /// El entorno rechazó la operación: scope/campo no existe al leer, o se
    /// intentó escribir fuera de `Locals`.
    Entorno,
}

/// Un error de expresión con posición en el texto fuente, para mensajes
/// útiles (estilo `expected 'and' but found '<' at offset 23`).
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorExpr {
    pub kind: ErrorKind,
    /// Desplazamiento en bytes dentro del texto fuente (0-based).
    pub pos: usize,
    /// Longitud en bytes del token ofensivo (0 si no aplica).
    pub len: usize,
    /// Mensaje humano, ya con contexto.
    pub mensaje: String,
}

impl ErrorExpr {
    pub fn sintaxis(pos: usize, len: usize, mensaje: impl Into<String>) -> Self {
        ErrorExpr { kind: ErrorKind::Sintaxis, pos, len, mensaje: mensaje.into() }
    }
    pub fn lexico(pos: usize, len: usize, mensaje: impl Into<String>) -> Self {
        ErrorExpr { kind: ErrorKind::Lexico, pos, len, mensaje: mensaje.into() }
    }
    pub fn tipo(pos: usize, mensaje: impl Into<String>) -> Self {
        ErrorExpr { kind: ErrorKind::Tipo, pos, len: 0, mensaje: mensaje.into() }
    }
    pub fn evaluacion(pos: usize, mensaje: impl Into<String>) -> Self {
        ErrorExpr { kind: ErrorKind::Evaluacion, pos, len: 0, mensaje: mensaje.into() }
    }
    pub fn entorno(pos: usize, mensaje: impl Into<String>) -> Self {
        ErrorExpr { kind: ErrorKind::Entorno, pos, len: 0, mensaje: mensaje.into() }
    }
}

impl std::fmt::Display for ErrorExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "en offset {}: {}", self.pos, self.mensaje)
    }
}

impl std::error::Error for ErrorExpr {}