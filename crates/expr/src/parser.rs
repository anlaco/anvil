//! Parser recursive-descent del lenguaje de expresiones de anvil.
//!
//! Sin dependencias externas (ADR-0001): escrito a mano sobre el lexer. El
//! subconjunto MVP separa **expresiones** (para precondiciones y futuras
//! postcondiciones) de **sentencias** (bloque `asigna` y paso `statement`).
//!
//! Sintaxis de **Julia** (estándar técnico moderno, divergencia deliberada de
//! TestStand C-like): `&&`/`||`/`!`, `nothing`, comparaciones **encadenables**
//! (`1 < x < 10` ≡ `1 < x && x < 10`), sin truthiness implícita.
//!
//! Precedencia (de menor a mayor, igual que Julia):
//! ```text
//! ||          (asociativo izq, cortocircuito)
//! &&          (asociativo izq, cortocircuito)
//! == != < > <= >=   (encadenables: a < b < c ≡ (a<b) && (b<c))
//! + -         (asociativo izq)
//! * /         (asociativo izq)
//! - !         (unarios prefijo)
//! . (campo)   (postfijo)
//! átomo
//! ```

use crate::ast::{BinOp, Expresion, Scope, Sentencia, UnOp};
use crate::error::ErrorExpr;
use crate::lexer::{lex, Tok, TokKind};

// API pública ---------------------------------------------------------------

/// Parsea una **expresión** (p. ej. una precondición). Termina con `Eof`;
/// basura tras la expresión es error.
pub fn parse_expresion(src: &str) -> Result<Expresion, ErrorExpr> {
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0 };
    let e = p.expresion()?;
    p.expect_eof()?;
    Ok(e)
}

/// Parsea una o más **sentencias** separadas por `;` (bloque `asigna` o paso
/// `statement`). Admite una sola sentencia sin `;` final.
pub fn parse_sentencias(src: &str) -> Result<Vec<Sentencia>, ErrorExpr> {
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0 };
    let mut stmts = Vec::new();
    // Permite espacios en blanco (ya ignorados por el lexer) y cadena vacía.
    if p.is_eof() {
        return Err(ErrorExpr::sintaxis(p.cur().pos, 0, "se esperaba al menos una sentencia"));
    }
    loop {
        stmts.push(p.sentencia()?);
        if matches!(p.cur().kind, TokKind::PuntoComa) {
            p.advance();
            if p.is_eof() {
                // `;` final opcional: lo permitimos.
                break;
            }
            continue;
        }
        break;
    }
    p.expect_eof()?;
    Ok(stmts)
}

// Parser interno -------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn cur(&self) -> &Tok {
        // `Eof` siempre existe al final; `i` nunca pasa del final.
        &self.toks[self.i.min(self.toks.len() - 1)]
    }
    fn advance(&mut self) -> &Tok {
        let t = &self.toks[self.i];
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn is_eof(&self) -> bool {
        matches!(self.cur().kind, TokKind::Eof)
    }
    fn expect_eof(&mut self) -> Result<(), ErrorExpr> {
        if self.is_eof() {
            Ok(())
        } else {
            let t = self.cur();
            Err(ErrorExpr::sintaxis(
                t.pos,
                t.len.max(1),
                format!("token inesperado tras la expresión: '{}'", t.kind.repr()),
            ))
        }
    }

    // sentencia ::= scope "." ident "=" expresion
    fn sentencia(&mut self) -> Result<Sentencia, ErrorExpr> {
        let (scope, campo) = self.var_ref()?;
        let ig = self.cur();
        if !matches!(ig.kind, TokKind::Igual) {
            return Err(ErrorExpr::sintaxis(
                ig.pos,
                ig.len.max(1),
                format!("se esperaba '=' en la asignación, pero vino '{}'", ig.kind.repr()),
            ));
        }
        self.advance();
        let valor = self.expresion()?;
        Ok(Sentencia::Assign { scope, campo, valor })
    }

    /// `scope "." ident` — referencia a variable. Devuelve (scope, campo).
    fn var_ref(&mut self) -> Result<(Scope, String), ErrorExpr> {
        let t = self.cur().clone();
        let scope = match &t.kind {
            TokKind::Ident(s) => match Scope::de_nombre(s) {
                Some(sc) => sc,
                None => {
                    return Err(ErrorExpr::sintaxis(
                        t.pos,
                        t.len,
                        format!("se esperaba un scope (locals/parameters/file_globals/resultado), pero vino '{s}'"),
                    ));
                }
            },
            _ => {
                return Err(ErrorExpr::sintaxis(
                    t.pos,
                    t.len.max(1),
                    format!("se esperaba un scope, pero vino '{}'", t.kind.repr()),
                ));
            }
        };
        self.advance();
        self.expect_tok(TokKind::Punto)?;
        let campo_tok = self.cur().clone();
        let campo = match &campo_tok.kind {
            TokKind::Ident(s) => s.clone(),
            _ => {
                return Err(ErrorExpr::sintaxis(
                    campo_tok.pos,
                    campo_tok.len.max(1),
                    format!(
                        "se esperaba un nombre de campo tras '.', pero vino '{}'",
                        campo_tok.kind.repr()
                    ),
                ));
            }
        };
        self.advance();
        Ok((scope, campo))
    }

    fn expect_tok(&mut self, want: TokKind) -> Result<(), ErrorExpr> {
        let t = self.cur().clone();
        if t.kind == want {
            self.advance();
            Ok(())
        } else {
            Err(ErrorExpr::sintaxis(
                t.pos,
                t.len.max(1),
                format!("se esperaba '{}', pero vino '{}'", want.repr(), t.kind.repr()),
            ))
        }
    }

    // expresion ::= or_expr
    fn expresion(&mut self) -> Result<Expresion, ErrorExpr> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<Expresion, ErrorExpr> {
        let mut izq = self.and_expr()?;
        while matches!(self.cur().kind, TokKind::OrOr) {
            self.advance();
            let der = self.and_expr()?;
            izq = Expresion::BinOp { op: BinOp::Or, izq: Box::new(izq), der: Box::new(der) };
        }
        Ok(izq)
    }

    fn and_expr(&mut self) -> Result<Expresion, ErrorExpr> {
        let mut izq = self.cmp_expr()?;
        while matches!(self.cur().kind, TokKind::AndAnd) {
            self.advance();
            let der = self.cmp_expr()?;
            izq = Expresion::BinOp { op: BinOp::And, izq: Box::new(izq), der: Box::new(der) };
        }
        Ok(izq)
    }

    // cmp_expr ::= add_expr (op_cmp add_expr)*   — encadenable (sintaxis Julia):
    // `a < b < c` ≡ `(a < b) && (b < c)`. Los operandos intermedios se reevalúan;
    // es seguro porque las expresiones son puras (sin efectos).
    fn cmp_expr(&mut self) -> Result<Expresion, ErrorExpr> {
        let mut operandos = vec![self.add_expr()?];
        let mut ops: Vec<BinOp> = Vec::new();
        loop {
            let op = match self.cur().kind {
                TokKind::EqEq => BinOp::Eq,
                TokKind::Neq => BinOp::Ne,
                TokKind::Lt => BinOp::Lt,
                TokKind::Le => BinOp::Le,
                TokKind::Gt => BinOp::Gt,
                TokKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            ops.push(op);
            operandos.push(self.add_expr()?);
        }
        if ops.is_empty() {
            // Una sola expresión: no hay comparación.
            return Ok(operandos.pop().unwrap());
        }
        // Cadena: (a op0 b) && (b op1 c) && ... reutilizando los operandos
        // intermedios como nodos AST (se reevalúan, pero son puros).
        let mut it = operandos.into_iter();
        let mut izq = it.next().unwrap();
        let mut resultado: Option<Expresion> = None;
        for op in ops {
            let der = it.next().unwrap();
            let cmp =
                Expresion::BinOp { op, izq: Box::new(izq.clone()), der: Box::new(der.clone()) };
            resultado = Some(match resultado {
                None => cmp,
                Some(acc) => {
                    Expresion::BinOp { op: BinOp::And, izq: Box::new(acc), der: Box::new(cmp) }
                }
            });
            izq = der;
        }
        Ok(resultado.unwrap())
    }

    fn add_expr(&mut self) -> Result<Expresion, ErrorExpr> {
        let mut izq = self.mul_expr()?;
        loop {
            let op = match self.cur().kind {
                TokKind::Mas => BinOp::Add,
                TokKind::Menos => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let der = self.mul_expr()?;
            izq = Expresion::BinOp { op, izq: Box::new(izq), der: Box::new(der) };
        }
        Ok(izq)
    }

    fn mul_expr(&mut self) -> Result<Expresion, ErrorExpr> {
        let mut izq = self.unary()?;
        loop {
            let op = match self.cur().kind {
                TokKind::Por => BinOp::Mul,
                TokKind::Div => BinOp::Div,
                _ => break,
            };
            self.advance();
            let der = self.unary()?;
            izq = Expresion::BinOp { op, izq: Box::new(izq), der: Box::new(der) };
        }
        Ok(izq)
    }

    // Unarios prefijo (Julia): `-x` (negación aritmética) y `!x` (negación
    // lógica). Liga más fuerte que `*`/`/` (igual que Julia).
    fn unary(&mut self) -> Result<Expresion, ErrorExpr> {
        match self.cur().kind {
            TokKind::Menos => {
                self.advance();
                let operando = self.unary()?;
                Ok(Expresion::UnOp { op: UnOp::Neg, operando: Box::new(operando) })
            }
            TokKind::Not => {
                self.advance();
                let operando = self.unary()?;
                Ok(Expresion::UnOp { op: UnOp::Not, operando: Box::new(operando) })
            }
            _ => self.postfix(),
        }
    }

    // En el MVP no hay acceso a campo sobre el resultado de una expresión;
    // `postfix` == `atom`. Se deja separado por simetría con post-MVP (encadenar).
    fn postfix(&mut self) -> Result<Expresion, ErrorExpr> {
        self.atom()
    }

    fn atom(&mut self) -> Result<Expresion, ErrorExpr> {
        let t = self.cur().clone();
        match &t.kind {
            TokKind::Numero(x) => {
                self.advance();
                Ok(Expresion::Lit(crate::value::Value::Numero(*x)))
            }
            TokKind::Bool(b) => {
                self.advance();
                Ok(Expresion::Lit(crate::value::Value::Bool(*b)))
            }
            TokKind::Nothing => {
                self.advance();
                Ok(Expresion::Lit(crate::value::Value::Nulo))
            }
            TokKind::Texto(s) => {
                self.advance();
                Ok(Expresion::Lit(crate::value::Value::Texto(s.clone())))
            }
            TokKind::Ident(s) => {
                // scope "." ident
                if let Some(scope) = Scope::de_nombre(s) {
                    self.advance();
                    self.expect_tok(TokKind::Punto)?;
                    let campo = self.ident_name()?;
                    Ok(Expresion::Var { scope, campo })
                } else {
                    Err(ErrorExpr::sintaxis(
                        t.pos,
                        t.len,
                        format!("'{s}' no es un scope (un átomo es un literal o scope.campo)"),
                    ))
                }
            }
            TokKind::ParentAp => {
                self.advance();
                let e = self.expresion()?;
                self.expect_tok(TokKind::ParentCierr)?;
                Ok(e)
            }
            TokKind::Eof => Err(ErrorExpr::sintaxis(
                t.pos,
                0,
                "fin de texto inesperado (se esperaba una expresión)",
            )),
            _ => Err(ErrorExpr::sintaxis(
                t.pos,
                t.len.max(1),
                format!("token inesperado '{}': se esperaba una expresión", t.kind.repr()),
            )),
        }
    }

    fn ident_name(&mut self) -> Result<String, ErrorExpr> {
        let t = self.cur().clone();
        match &t.kind {
            TokKind::Ident(s) => {
                self.advance();
                Ok(s.clone())
            }
            _ => Err(ErrorExpr::sintaxis(
                t.pos,
                t.len.max(1),
                format!("se esperaba un nombre de campo, pero vino '{}'", t.kind.repr()),
            )),
        }
    }
}

// Repr de tokens para mensajes de error -------------------------------------

impl TokKind {
    fn repr(&self) -> String {
        match self {
            TokKind::Numero(x) => format!("{x}"),
            TokKind::Bool(b) => format!("{b}"),
            TokKind::Nothing => "nothing".into(),
            TokKind::Ident(s) => s.clone(),
            TokKind::Texto(s) => format!("\"{s}\""),
            TokKind::AndAnd => "&&".into(),
            TokKind::OrOr => "||".into(),
            TokKind::Not => "!".into(),
            TokKind::Igual => "=".into(),
            TokKind::EqEq => "==".into(),
            TokKind::Neq => "!=".into(),
            TokKind::Lt => "<".into(),
            TokKind::Le => "<=".into(),
            TokKind::Gt => ">".into(),
            TokKind::Ge => ">=".into(),
            TokKind::Mas => "+".into(),
            TokKind::Menos => "-".into(),
            TokKind::Por => "*".into(),
            TokKind::Div => "/".into(),
            TokKind::Punto => ".".into(),
            TokKind::ParentAp => "(".into(),
            TokKind::ParentCierr => ")".into(),
            TokKind::Coma => ",".into(),
            TokKind::PuntoComa => ";".into(),
            TokKind::Eof => "<fin>".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp, Expresion, Scope, Sentencia, UnOp};
    use crate::value::Value;

    fn var(scope: Scope, campo: &str) -> Expresion {
        Expresion::Var { scope, campo: campo.into() }
    }
    fn lit_num(x: f64) -> Expresion {
        Expresion::Lit(Value::Numero(x))
    }
    fn binop(op: BinOp, izq: Expresion, der: Expresion) -> Expresion {
        Expresion::BinOp { op, izq: Box::new(izq), der: Box::new(der) }
    }

    #[test]
    fn precedencia_aritmetica() {
        // 2 + 3 * 4  →  Add(2, Mul(3,4))
        let e = parse_expresion("2 + 3 * 4").unwrap();
        assert_eq!(
            e,
            binop(BinOp::Add, lit_num(2.0), binop(BinOp::Mul, lit_num(3.0), lit_num(4.0)))
        );
        // 2 * 3 + 4  →  Add(Mul(2,3), 4)
        let e = parse_expresion("2 * 3 + 4").unwrap();
        assert_eq!(
            e,
            binop(BinOp::Add, binop(BinOp::Mul, lit_num(2.0), lit_num(3.0)), lit_num(4.0))
        );
    }

    #[test]
    fn precedencia_logica() {
        // a || b && c  →  Or(a, And(b, c))   (|| más flojo que &&, como Julia)
        let e = parse_expresion("locals.a || locals.b && locals.c").unwrap();
        assert_eq!(
            e,
            binop(
                BinOp::Or,
                var(Scope::Locals, "a"),
                binop(BinOp::And, var(Scope::Locals, "b"), var(Scope::Locals, "c"))
            )
        );
    }

    #[test]
    fn not_liga_mas_fuerte_que_and() {
        // !a && b  →  And(Not(a), b)   (! liga más fuerte que &&, como Julia)
        let e = parse_expresion("!locals.a && locals.b").unwrap();
        assert_eq!(
            e,
            binop(
                BinOp::And,
                Expresion::UnOp { op: UnOp::Not, operando: Box::new(var(Scope::Locals, "a")) },
                var(Scope::Locals, "b")
            )
        );
    }

    #[test]
    fn comparaciones_encadenadas_como_julia() {
        // 1 < x && x < 10  ≡  1 < x < 10  (azúcar Julia/Python)
        let e = parse_expresion("1 < locals.x < 10").unwrap();
        assert_eq!(
            e,
            binop(
                BinOp::And,
                binop(BinOp::Lt, lit_num(1.0), var(Scope::Locals, "x")),
                binop(BinOp::Lt, var(Scope::Locals, "x"), lit_num(10.0))
            )
        );
    }

    #[test]
    fn parentesis_agrupan() {
        let e = parse_expresion("(1 + 2) * 3").unwrap();
        assert_eq!(
            e,
            binop(BinOp::Mul, binop(BinOp::Add, lit_num(1.0), lit_num(2.0)), lit_num(3.0))
        );
    }

    #[test]
    fn acceso_a_scope_con_campo() {
        let e = parse_expresion("resultado.valor_medido").unwrap();
        assert_eq!(e, var(Scope::Resultado, "valor_medido"));
    }

    #[test]
    fn scope_sin_campo_es_error() {
        assert!(parse_expresion("locals").is_err());
        assert!(parse_expresion("locals.").is_err());
    }

    #[test]
    fn palabra_clave_como_campo_es_error() {
        // `nothing` es palabra clave, no puede ser campo.
        assert!(parse_expresion("locals.nothing").is_err());
    }

    #[test]
    fn literales_nulo_true_false_texto() {
        assert!(matches!(parse_expresion("nothing").unwrap(), Expresion::Lit(Value::Nulo)));
        assert!(matches!(parse_expresion("true").unwrap(), Expresion::Lit(Value::Bool(true))));
        let e = parse_expresion("\"paso\"").unwrap();
        assert_eq!(e, Expresion::Lit(Value::Texto("paso".into())));
    }

    #[test]
    fn negativo_unario() {
        let e = parse_expresion("-5").unwrap();
        assert!(matches!(e, Expresion::UnOp { op: UnOp::Neg, .. }));
    }

    #[test]
    fn basura_tras_expresion_es_error() {
        assert!(parse_expresion("1 + 2 3").is_err());
    }

    #[test]
    fn asignacion_se_parsea() {
        let stmts = parse_sentencias("locals.x = resultado.valor_medido").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(
            matches!(&stmts[0], Sentencia::Assign { scope: Scope::Locals, campo, valor: _ } if campo == "x")
        );
    }

    #[test]
    fn varias_sentencias_separadas_por_punto_coma() {
        let stmts = parse_sentencias("locals.a = 1; locals.b = 2").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn expresion_sin_asignacion_no_es_sentencia() {
        assert!(parse_sentencias("locals.x").is_err());
    }

    #[test]
    fn error_de_sintaxis_tiene_posicion() {
        let err = parse_expresion("1 +").unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::Sintaxis);
        assert!(err.pos > 0);
    }
}
