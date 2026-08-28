//! Evaluator del lenguaje de expresiones de anvil.
//!
//! El engine no conoce `ResultadoStep` ni el dominio (ADR-0005): opera sobre
//! [`Value`] y un trait [`Entorno`] que el motor implementa. Las reglas de tipo
//! son estrictas — sin coerción silenciosa ni truthiness implícita (deliberado:
//! evita bugs tipo Python en un lenguaje de datos de test). El engine
//! **nunca pánico**: todo devuelve `Result`.

use crate::ast::{BinOp, Expresion, Scope, Sentencia, UnOp};
use crate::error::ErrorExpr;
use crate::value::Value;
use Value::{Bool, Nulo, Numero, Texto};

/// El entorno que el motor implementa. El engine sólo lee/escribe `Value`
/// por `(scope, campo)`.
pub trait Entorno {
    /// Lee una variable. `Err` si el scope/campo no existe (política del
    /// motor: `resultado.*` es laxa → `Nulo`; `locals`/`parameters`/
    /// `file_globals` es estricta).
    fn lee(&self, scope: Scope, campo: &str) -> Result<Value, ErrorExpr>;

    /// Escribe un valor. En el MVP sólo se permite `Locals`; el motor devuelve
    /// `Err` si `scope != Locals` (regla "sólo se muta Locals").
    fn escribe(&mut self, scope: Scope, campo: &str, valor: Value) -> Result<(), ErrorExpr>;
}

/// Evalúa una expresión (sólo lectura: toma `&impl Entorno`). Para
/// precondiciones y futuras postcondiciones.
pub fn eval(expr: &Expresion, env: &impl Entorno) -> Result<Value, ErrorExpr> {
    match expr {
        Expresion::Lit(v) => Ok(v.clone()),
        Expresion::Var { scope, campo } => env.lee(*scope, campo),
        Expresion::BinOp { op, izq, der } => {
            // Cortocircuito para `and`/`or`: si el lado izquierdo decide, no
            // evaluamos el derecho (evita errores de tipo/evaluación del lado
            // derecho cuando ya no hace falta).
            match op {
                BinOp::And => {
                    let l = eval(izq, env)?;
                    match l {
                        // Cortocircuito: `false and der` no evalúa `der`.
                        Value::Bool(false) => Ok(Value::Bool(false)),
                        Value::Bool(true) => {
                            let r = eval(der, env)?;
                            if let Value::Bool(rb) = r {
                                Ok(Value::Bool(rb))
                            } else {
                                Err(ErrorExpr::tipo(
                                    0,
                                    format!("'and' necesita bool, no {}", r.tipo()),
                                ))
                            }
                        }
                        _ => Err(ErrorExpr::tipo(
                            0,
                            format!("'and' necesita bool, no {}", l.tipo()),
                        )),
                    }
                }
                BinOp::Or => {
                    let l = eval(izq, env)?;
                    match l {
                        // Cortocircuito: `true or der` no evalúa `der`.
                        Value::Bool(true) => Ok(Value::Bool(true)),
                        Value::Bool(false) => {
                            let r = eval(der, env)?;
                            if let Value::Bool(rb) = r {
                                Ok(Value::Bool(rb))
                            } else {
                                Err(ErrorExpr::tipo(
                                    0,
                                    format!("'or' necesita bool, no {}", r.tipo()),
                                ))
                            }
                        }
                        _ => Err(ErrorExpr::tipo(
                            0,
                            format!("'or' necesita bool, no {}", l.tipo()),
                        )),
                    }
                }
                _ => {
                    let l = eval(izq, env)?;
                    let r = eval(der, env)?;
                    eval_bin(op, l, r)
                }
            }
        }
        Expresion::UnOp { op, operando } => {
            let v = eval(operando, env)?;
            eval_un(*op, v)
        }
    }
}

/// Ejecuta una lista de sentencias (escritura: toma `&mut impl Entorno`). Para
/// el bloque `asigna` y el paso `statement`.
pub fn eval_sentencias(stmts: &[Sentencia], env: &mut impl Entorno) -> Result<(), ErrorExpr> {
    for s in stmts {
        match s {
            Sentencia::Assign {
                scope,
                campo,
                valor,
            } => {
                let v = eval(valor, env)?;
                env.escribe(*scope, campo, v)?;
            }
        }
    }
    Ok(())
}

fn eval_un(op: UnOp, v: Value) -> Result<Value, ErrorExpr> {
    match (op, &v) {
        (UnOp::Neg, Value::Numero(x)) => Ok(Value::Numero(-x)),
        (UnOp::Neg, _) => Err(ErrorExpr::tipo(
            0,
            format!("'- ' necesita número, no {}", v.tipo()),
        )),
        (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnOp::Not, _) => Err(ErrorExpr::tipo(
            0,
            format!("'not' necesita bool, no {}", v.tipo()),
        )),
    }
}

fn eval_bin(op: &BinOp, l: Value, r: Value) -> Result<Value, ErrorExpr> {
    match op {
        // Aritmética: sólo números. Nulo → error (no se mezcla en aritmética).
        BinOp::Add => aritm(&l, &r, |a, b| a + b, "+"),
        BinOp::Sub => aritm(&l, &r, |a, b| a - b, "-"),
        BinOp::Mul => aritm(&l, &r, |a, b| a * b, "*"),
        BinOp::Div => {
            let (a, b) = num_num(&l, &r, "/")?;
            if b == 0.0 {
                Err(ErrorExpr::evaluacion(0, "división por cero"))
            } else {
                Ok(Numero(a / b))
            }
        }
        // Orden: sólo números.
        BinOp::Lt => orden(&l, &r, |a, b| a < b, "<"),
        BinOp::Le => orden(&l, &r, |a, b| a <= b, "<="),
        BinOp::Gt => orden(&l, &r, |a, b| a > b, ">"),
        BinOp::Ge => orden(&l, &r, |a, b| a >= b, ">="),
        // Igualdad: mismo tipo compara; tipos distintos → false/true (no error).
        // Nulo == Nulo → true; Nulo == otra cosa → false.
        //
        // A reference is the one exception, and it is an error and not a
        // `false`: comparing handles is an operation on a value the engine
        // cannot read, and answering `false` to `locals.rack == locals.dmm`
        // would be answering a question nobody can answer (ADR-0022 §1).
        BinOp::Eq => sin_referencia(&l, &r, "==").map(|_| Bool(iguales(&l, &r))),
        BinOp::Ne => sin_referencia(&l, &r, "!=").map(|_| Bool(!iguales(&l, &r))),
        // And/Or se gestionan por cortocircuito en `eval`; no llegan aquí.
        BinOp::And | BinOp::Or => unreachable!("and/or se cortocircuitan en eval"),
    }
}

/// Refuses an operator either of whose operands is a reference.
///
/// The refusal is the whole point of the type (ADR-0022 §1): a handle can be
/// read out of a variable and handed to a step, and nothing else. Arithmetic
/// and ordering already refuse it by demanding numbers; equality would not,
/// which is why it is spelled out here.
fn sin_referencia(l: &Value, r: &Value, op: &str) -> Result<(), ErrorExpr> {
    for v in [l, r] {
        if let Value::Reference(rf) = v {
            return Err(ErrorExpr::tipo(
                0,
                format!(
                    "'{op}' no se puede aplicar a una referencia ({}): una referencia \
                     es opaca — se lee de una variable y se pasa a un paso, y no \
                     admite ninguna otra operación",
                    rf.mostrar()
                ),
            ));
        }
    }
    Ok(())
}

fn num_num(l: &Value, r: &Value, op: &str) -> Result<(f64, f64), ErrorExpr> {
    match (l, r) {
        (Numero(a), Numero(b)) => Ok((*a, *b)),
        (Numero(_), _) => Err(ErrorExpr::tipo(
            0,
            format!("'{op}' necesita número, no {}", r.tipo()),
        )),
        _ => Err(ErrorExpr::tipo(
            0,
            format!("'{op}' necesita número, no {}", l.tipo()),
        )),
    }
}

fn aritm<F: Fn(f64, f64) -> f64>(l: &Value, r: &Value, f: F, op: &str) -> Result<Value, ErrorExpr> {
    // Nulo explícito: una medida ausente no debe entrar en aritmética.
    if matches!(l, Value::Nulo) || matches!(r, Value::Nulo) {
        return Err(ErrorExpr::evaluacion(
            0,
            format!("'{}' con un valor nulo (usa '!= nothing' antes)", op),
        ));
    }
    let (a, b) = num_num(l, r, op)?;
    Ok(Value::Numero(f(a, b)))
}

fn orden<F: Fn(f64, f64) -> bool>(
    l: &Value,
    r: &Value,
    f: F,
    op: &str,
) -> Result<Value, ErrorExpr> {
    if matches!(l, Value::Nulo) || matches!(r, Value::Nulo) {
        return Err(ErrorExpr::evaluacion(
            0,
            format!("'{}' con un valor nulo (usa '!= nothing' antes)", op),
        ));
    }
    let (a, b) = num_num(l, r, op)?;
    Ok(Value::Bool(f(a, b)))
}

fn iguales(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Numero(a), Numero(b)) => a == b,
        (Bool(a), Bool(b)) => a == b,
        (Texto(a), Texto(b)) => a == b,
        (Nulo, Nulo) => true,
        // References never reach here: `sin_referencia` refuses them before
        // the comparison is attempted.
        // Tipos distintos (incl. Nulo vs otra cosa) → no iguales.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp, Expresion, Scope, Sentencia, UnOp};
    use crate::value::Value;
    use std::collections::HashMap;

    /// Entorno de test: mapa de (scope, campo) → valor. Rechaza escribir fuera
    /// de Locals.
    struct EntornoMock {
        datos: HashMap<(Scope, String), Value>,
        escrituras: Vec<(Scope, String, Value)>,
    }
    impl EntornoMock {
        fn new() -> Self {
            EntornoMock {
                datos: HashMap::new(),
                escrituras: Vec::new(),
            }
        }
        fn set(mut self, scope: Scope, campo: &str, v: Value) -> Self {
            self.datos.insert((scope, campo.into()), v);
            self
        }
    }
    impl Entorno for EntornoMock {
        fn lee(&self, scope: Scope, campo: &str) -> Result<Value, ErrorExpr> {
            self.datos
                .get(&(scope, campo.to_string()))
                .cloned()
                .ok_or_else(|| {
                    ErrorExpr::entorno(0, format!("no existe '{}.{}'", scope.nombre(), campo))
                })
        }
        fn escribe(&mut self, scope: Scope, campo: &str, valor: Value) -> Result<(), ErrorExpr> {
            if scope != Scope::Locals {
                return Err(ErrorExpr::entorno(
                    0,
                    format!("no se puede escribir en '{}'", scope.nombre()),
                ));
            }
            self.escrituras.push((scope, campo.into(), valor.clone()));
            self.datos.insert((scope, campo.into()), valor);
            Ok(())
        }
    }

    fn num(x: f64) -> Expresion {
        Expresion::Lit(Value::Numero(x))
    }
    fn var(scope: Scope, campo: &str) -> Expresion {
        Expresion::Var {
            scope,
            campo: campo.into(),
        }
    }
    fn bin(op: BinOp, l: Expresion, r: Expresion) -> Expresion {
        Expresion::BinOp {
            op,
            izq: Box::new(l),
            der: Box::new(r),
        }
    }

    #[test]
    fn aritmetica_basica() {
        let env = EntornoMock::new();
        assert_eq!(
            eval(&bin(BinOp::Add, num(2.0), num(3.0)), &env).unwrap(),
            Value::Numero(5.0)
        );
        assert_eq!(
            eval(&bin(BinOp::Mul, num(2.0), num(3.0)), &env).unwrap(),
            Value::Numero(6.0)
        );
    }

    #[test]
    fn division_por_cero_es_error() {
        let env = EntornoMock::new();
        assert!(eval(&bin(BinOp::Div, num(1.0), num(0.0)), &env).is_err());
    }

    #[test]
    fn igualdad_entre_tipos_distintos_es_false_sin_error() {
        let env = EntornoMock::new();
        assert_eq!(
            eval(
                &bin(
                    BinOp::Eq,
                    num(1.0),
                    Expresion::Lit(Value::Texto("1".into()))
                ),
                &env
            )
            .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval(
                &bin(
                    BinOp::Ne,
                    num(1.0),
                    Expresion::Lit(Value::Texto("1".into()))
                ),
                &env
            )
            .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn nulo_igual_nulo_es_true() {
        let env = EntornoMock::new();
        assert_eq!(
            eval(
                &bin(
                    BinOp::Eq,
                    Expresion::Lit(Value::Nulo),
                    Expresion::Lit(Value::Nulo)
                ),
                &env
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval(&bin(BinOp::Ne, Expresion::Lit(Value::Nulo), num(5.0)), &env).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn nulo_en_aritmetica_es_error() {
        let env = EntornoMock::new();
        let e = bin(BinOp::Add, Expresion::Lit(Value::Nulo), num(1.0));
        assert!(eval(&e, &env).is_err());
    }

    #[test]
    fn nulo_en_orden_es_error() {
        let env = EntornoMock::new();
        let e = bin(BinOp::Lt, Expresion::Lit(Value::Nulo), num(1.0));
        assert!(eval(&e, &env).is_err());
    }

    #[test]
    fn and_or_not_solo_bool() {
        let env = EntornoMock::new()
            .set(Scope::Locals, "a", Value::Bool(true))
            .set(Scope::Locals, "b", Value::Bool(false));
        assert_eq!(
            eval(
                &bin(BinOp::And, var(Scope::Locals, "a"), var(Scope::Locals, "b")),
                &env
            )
            .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval(
                &bin(BinOp::Or, var(Scope::Locals, "a"), var(Scope::Locals, "b")),
                &env
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval(
                &Expresion::UnOp {
                    op: UnOp::Not,
                    operando: Box::new(var(Scope::Locals, "b"))
                },
                &env
            )
            .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn and_or_con_numero_es_error_de_tipo() {
        let env = EntornoMock::new().set(Scope::Locals, "x", Value::Numero(1.0));
        assert!(eval(
            &bin(BinOp::And, var(Scope::Locals, "x"), var(Scope::Locals, "x")),
            &env
        )
        .is_err());
    }

    #[test]
    fn cortocircuito_and_no_evalua_derecho_cuando_izquierdo_es_false() {
        // `false and locals.inexistente` no debe errorar: el derecho no se evalúa.
        let env = EntornoMock::new();
        let e = bin(
            BinOp::And,
            Expresion::Lit(Value::Bool(false)),
            var(Scope::Locals, "inexistente"),
        );
        assert_eq!(eval(&e, &env).unwrap(), Value::Bool(false));
    }

    #[test]
    fn cortocircuito_or_no_evalua_derecho_cuando_izquierdo_es_true() {
        let env = EntornoMock::new();
        let e = bin(
            BinOp::Or,
            Expresion::Lit(Value::Bool(true)),
            var(Scope::Locals, "inexistente"),
        );
        assert_eq!(eval(&e, &env).unwrap(), Value::Bool(true));
    }

    #[test]
    fn acceso_a_scopes_lee_del_entorno() {
        let env = EntornoMock::new().set(Scope::FileGlobals, "lote", Value::Texto("A".into()));
        assert_eq!(
            eval(&var(Scope::FileGlobals, "lote"), &env).unwrap(),
            Value::Texto("A".into())
        );
    }

    #[test]
    fn asigna_escribe_en_locals() {
        let mut env = EntornoMock::new().set(Scope::Resultado, "valor_medido", Value::Numero(4.2));
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Locals,
            campo: "x".into(),
            valor: var(Scope::Resultado, "valor_medido"),
        }];
        eval_sentencias(&stmts, &mut env).unwrap();
        assert_eq!(env.lee(Scope::Locals, "x").unwrap(), Value::Numero(4.2));
    }

    #[test]
    fn asigna_a_parameters_es_error() {
        let mut env = EntornoMock::new();
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Parameters,
            campo: "x".into(),
            valor: num(1.0),
        }];
        assert!(eval_sentencias(&stmts, &mut env).is_err());
    }

    fn referencia() -> Value {
        Value::Reference(crate::Reference {
            executor: "banco".into(),
            lifetime: "l1".into(),
            payload: "s1".into(),
        })
    }

    /// **Criterio 3 del encargo**, en el sitio donde se decide: una referencia
    /// no entra en ninguna operación (ADR-0022 §1).
    ///
    /// La aritmética y el orden ya la rechazaban por exigir números; la
    /// igualdad **no**, y ésa es la que hay que sujetar a mano: `==` contestaba
    /// `false` a tipos distintos, así que `locals.rack == 'abc'` habría dado un
    /// booleano perfectamente utilizable como veredicto.
    ///
    /// Visto fallar quitando `sin_referencia` de `Eq`/`Ne`: los dos últimos
    /// casos pasan a devolver `Ok(Bool(false))` y el test se pone rojo.
    #[test]
    fn una_referencia_no_entra_en_ninguna_operacion() {
        let env = EntornoMock::new()
            .set(Scope::Locals, "rack", referencia())
            .set(Scope::Locals, "n", Value::Numero(2.0));
        for (op, texto) in [
            (BinOp::Add, "+"),
            (BinOp::Sub, "-"),
            (BinOp::Mul, "*"),
            (BinOp::Div, "/"),
            (BinOp::Lt, "<"),
            (BinOp::Ge, ">="),
            (BinOp::Eq, "=="),
            (BinOp::Ne, "!="),
        ] {
            let e = Expresion::BinOp {
                op,
                izq: Box::new(var(Scope::Locals, "rack")),
                der: Box::new(var(Scope::Locals, "n")),
            };
            let err = eval(&e, &env)
                .expect_err(&format!("'{texto}' sobre una referencia tiene que fallar"))
                .to_string();
            assert!(
                err.contains("referencia"),
                "'{texto}': el error tiene que decir que es una referencia: {err}"
            );
        }
    }

    /// `not` y `and`/`or` la rechazan por exigir bool, y se comprueba para que
    /// una referencia no pueda ser jamás el veredicto de un `pass_fail`.
    #[test]
    fn una_referencia_no_puede_ser_un_veredicto() {
        let env = EntornoMock::new().set(Scope::Locals, "rack", referencia());
        let no = Expresion::UnOp {
            op: UnOp::Not,
            operando: Box::new(var(Scope::Locals, "rack")),
        };
        assert!(eval(&no, &env).is_err());
        let y = Expresion::BinOp {
            op: BinOp::And,
            izq: Box::new(var(Scope::Locals, "rack")),
            der: Box::new(Expresion::Lit(Value::Bool(true))),
        };
        assert!(eval(&y, &env).is_err());
    }

    /// Leerla y copiarla **sí** funciona, y tiene que seguir funcionando: son
    /// las dos cosas que hacen `assign` e `inputs` (ADR-0022 §1).
    #[test]
    fn una_referencia_se_lee_y_se_copia() {
        let mut env = EntornoMock::new().set(Scope::Locals, "rack", referencia());
        assert_eq!(
            eval(&var(Scope::Locals, "rack"), &env).unwrap(),
            referencia()
        );
        eval_sentencias(
            &[Sentencia::Assign {
                scope: Scope::Locals,
                campo: "otro".into(),
                valor: var(Scope::Locals, "rack"),
            }],
            &mut env,
        )
        .unwrap();
        assert_eq!(env.lee(Scope::Locals, "otro").unwrap(), referencia());
    }
}
