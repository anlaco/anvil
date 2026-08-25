//! Motor de expresiones de anvil (RF-35).
//!
//! Lenguaje de expresiones acotado: asignación con `=`, comparaciones
//! `== != < > <= >=`, lógicos **`&&` / `||` / `!`**, acceso a campos con `.`
//! (`locals.voltaje_leido`,
//! `resultado.valor_medido`). Subconjunto MVP: aritmética `+ - * /`, lógica y
//! comparación, acceso a scopes y al resultado del paso, asignación a `Locals`.
//!
//! Es un AST acotado, **no** un lenguaje Turing-completo: si hace falta lógica
//! compleja, va en un paso (código real, ADR-0003). Sin dependencias externas
//! pesadas, para compilar a WASM (ADR-0001).
//!
//! El engine no conoce `modelo::ResultadoStep` ni el dominio (ADR-0005): opera
//! sobre [`Value`] y un trait [`Entorno`] que el motor implementa. El cargador
//! parsea las expresiones a AST al cargar (fail-fast); el motor las evalúa en
//! runtime.

pub mod ast;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

/// El único campo de `resultado` que es un mapa y no un escalar, y por tanto
/// el único que admite un nivel más: `resultado.salidas.<nombre>` (ADR-0020).
///
/// Vive aquí y no en `modelo` porque lo necesitan el parser (para admitir el
/// segundo punto) y el entorno del motor (para resolverlo), y `expr` no
/// depende de `modelo` — el engine no conoce el dominio (ADR-0005).
pub const CAMPO_SALIDAS: &str = "salidas";

pub use ast::{BinOp, Expresion, Scope, Sentencia, UnOp};
pub use error::{ErrorExpr, ErrorKind};
pub use eval::{eval, eval_sentencias, Entorno};
pub use parser::{parse_expresion, parse_sentencias};
pub use value::Value;
