//! Lexer del lenguaje de expresiones de anvil.
//!
//! Produce `Vec<Tok>` con la posición (byte offset) de cada token, para que el
//! parser emita errores posicionales. Las palabras clave `true`/`false`/
//! `nothing` se reconocen como tokens propios — **no** pueden usarse como
//! nombres de campo. Los operadores lógicos son los de **Julia**: `&&`/`||`/`!`
//! (no `and`/`or`/`not`); `nothing` representa la ausencia. Los scopes
//! (`locals`, `parameters`, `file_globals`, `resultado`) son identificadores
//! normales que el parser reconoce en posición de átomo seguido de `.`.

use crate::error::ErrorExpr;

/// Clase de token. Los números ya van como `f64`; los identificadores como
/// `String`.
#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    Numero(f64),
    Bool(bool),
    /// Palabra clave `nothing` (ausencia, como en Julia).
    Nothing,
    Ident(String),
    /// Literal de texto entre comillas dobles (`"pass"`).
    Texto(String),
    // Lógicos (sintaxis Julia: `&&`, `||`, `!`).
    AndAnd,
    OrOr,
    Not,
    // Asignación.
    Igual,
    // Comparación.
    EqEq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    // Aritmética.
    Mas,
    Menos,
    Por,
    Div,
    // Puntuación.
    Punto,
    ParentAp,
    ParentCierr,
    Coma,
    PuntoComa,
    Eof,
}

/// Un token con su posición en el texto fuente.
#[derive(Debug, Clone, PartialEq)]
pub struct Tok {
    pub kind: TokKind,
    /// Byte offset de inicio (0-based).
    pub pos: usize,
    /// Longitud en bytes.
    pub len: usize,
}

/// Tokeniza el texto. El último token es siempre `Eof`.
pub fn lex(src: &str) -> Result<Vec<Tok>, ErrorExpr> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut toks = Vec::new();

    while i < bytes.len() {
        let c = bytes[i];

        // Espacios en blanco.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;

        // Comentarios: `# ...` hasta fin de línea (útil en bloques largos).
        if c == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Números: dígitos, con parte decimal opcional. Sin notación científica
        // en el MVP (post-MVP si hace falta).
        if c.is_ascii_digit() || (c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'.' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
            }
            let texto = &src[i..j];
            let n: f64 = texto.parse().map_err(|_| {
                ErrorExpr::lexico(start, j - start, format!("número inválido: '{texto}'"))
            })?;
            toks.push(Tok {
                kind: TokKind::Numero(n),
                pos: start,
                len: j - start,
            });
            i = j;
            continue;
        }

        // Identificadores y palabras clave (ASCII alfanum + guion bajo; el
        // primer carácter debe ser letra o guion bajo).
        if c.is_ascii_alphabetic() || c == b'_' {
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            let texto = &src[i..j];
            let kind = match texto {
                "true" => TokKind::Bool(true),
                "false" => TokKind::Bool(false),
                "nothing" => TokKind::Nothing,
                // `and`/`or`/`not` ya NO son palabras clave (sintaxis Julia:
                // `&&`/`||`/`!`). Si alguien las escribe, son identificadores
                // normales y el parser las rechazará (no son scopes).
                otro => TokKind::Ident(otro.to_string()),
            };
            toks.push(Tok {
                kind,
                pos: start,
                len: j - start,
            });
            i = j;
            continue;
        }

        // Strings: comillas dobles, sin escapes en el MVP (post-MVP: `\n`, `\"`).
        if c == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += 1;
            }
            if j >= bytes.len() {
                return Err(ErrorExpr::lexico(
                    start,
                    bytes.len() - start,
                    "string sin cerrar",
                ));
            }
            // j apunta a la comilla de cierre.
            let texto = src[i + 1..j].to_string();
            toks.push(Tok {
                kind: TokKind::Texto(texto),
                pos: start,
                len: j + 1 - start,
            });
            i = j + 1;
            continue;
        }

        // Símbolos de dos caracteres primero (==, !=, <=, >=, &&, ||).
        let dos = if i + 1 < bytes.len() {
            Some(&src[i..i + 2])
        } else {
            None
        };
        if let Some(dd) = dos {
            let kind = match dd {
                "==" => Some(TokKind::EqEq),
                "!=" => Some(TokKind::Neq),
                "<=" => Some(TokKind::Le),
                ">=" => Some(TokKind::Ge),
                "&&" => Some(TokKind::AndAnd),
                "||" => Some(TokKind::OrOr),
                _ => None,
            };
            if let Some(k) = kind {
                toks.push(Tok {
                    kind: k,
                    pos: start,
                    len: 2,
                });
                i += 2;
                continue;
            }
        }

        // Símbolos de un carácter.
        let kind = match c {
            b'=' => TokKind::Igual,
            b'!' => TokKind::Not,
            b'<' => TokKind::Lt,
            b'>' => TokKind::Gt,
            b'+' => TokKind::Mas,
            b'-' => TokKind::Menos,
            b'*' => TokKind::Por,
            b'/' => TokKind::Div,
            b'.' => TokKind::Punto,
            b'(' => TokKind::ParentAp,
            b')' => TokKind::ParentCierr,
            b',' => TokKind::Coma,
            b';' => TokKind::PuntoComa,
            _ => {
                return Err(ErrorExpr::lexico(
                    start,
                    1,
                    format!(
                        "carácter inesperado '{}'",
                        src[i..i + 1].chars().next().unwrap_or(' ')
                    ),
                ));
            }
        };
        toks.push(Tok {
            kind,
            pos: start,
            len: 1,
        });
        i += 1;
    }

    toks.push(Tok {
        kind: TokKind::Eof,
        pos: bytes.len(),
        len: 0,
    });
    Ok(toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        lex(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn numeros_enteros_y_decimales() {
        assert!(matches!(
            kinds("5").as_slice(),
            [TokKind::Numero(5.0), TokKind::Eof]
        ));
        assert!(matches!(
            kinds("4.2").as_slice(),
            [TokKind::Numero(_), TokKind::Eof]
        ));
    }

    #[test]
    fn palabras_clave_vs_identificadores() {
        // `true`/`false`/`nothing` son palabras clave; `and`/`or`/`not` ya NO
        // lo son (sintaxis Julia: `&&`/`||`/`!`) — son identificadores normales.
        let k = kinds("true false nothing and");
        assert!(matches!(
            k.as_slice(),
            [
                TokKind::Bool(true),
                TokKind::Bool(false),
                TokKind::Nothing,
                TokKind::Ident(_),
                TokKind::Eof
            ]
        ));
    }

    #[test]
    fn simbolos_dos_caracteres() {
        let k = kinds("== != <= >= && || < >");
        assert_eq!(k.len(), 9);
        assert!(matches!(k[0], TokKind::EqEq));
        assert!(matches!(k[1], TokKind::Neq));
        assert!(matches!(k[4], TokKind::AndAnd));
        assert!(matches!(k[5], TokKind::OrOr));
    }

    #[test]
    fn admiracion_es_not_unario() {
        let k = kinds("!x");
        assert!(matches!(k[0], TokKind::Not));
    }

    #[test]
    fn string_sin_cerrar_es_error() {
        assert!(lex("\"hola").is_err());
    }

    #[test]
    fn caracter_raro_es_error() {
        assert!(lex("@").is_err());
    }

    #[test]
    fn comentario_se_ignora() {
        let k = kinds("1 # comentario\n 2");
        assert_eq!(k.len(), 3);
    }
}
