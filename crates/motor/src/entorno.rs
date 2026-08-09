//! Entorno de variables del motor (RF-31).
//!
//! Implementa [`expr::Entorno`] sobre los scopes `locals`/`parameters`/
//! `file_globals` y el `resultado` del paso en curso. El engine no conoce
//! `ResultadoStep` ni el dominio (ADR-0005): sólo lee/escribe [`expr::Value`]
//! por `(scope, campo)`.
//!
//! Reglas (ver [variables-y-alcances.md](../../docs/diseno/variables-y-alcances.md)):
//! - Lectura: `resultado.*` es **laxa** (campo ausente → `Nulo`, porque
//!   `valor_medido` puede faltar legítimamente); `locals`/`parameters`/
//!   `file_globals` son **estrictas** (campo no declarado al cargar → error:
//!   una referencia a un local inexistente es un error de autor, fail-fast).
//! - Escritura: `Locals` siempre mutable. `Parameters` es mutable **sólo en
//!   una subsecuencia** (`parameters_mutables = true`): es el canal de
//!   retorno by-reference de un sequence call (M4b, ADR-0010). En la raíz,
//!   `parameters` es de sólo lectura (no hay a quién devolver). `FileGlobals`
//!   y `Resultado` nunca se escriben directamente (el motor setea `resultado`).

use std::collections::HashMap;

use expr::{Entorno, ErrorExpr, Scope, Value};
use modelo::{DefinicionSecuencia, ResultadoStep, ValorDefinicion};

/// El runtime de variables de una ejecución de secuencia. `Locals` mutable
/// (por `asigna`); `Parameters` mutable sólo si la secuencia es una
/// subsecuencia (`parameters_mutables`); `FileGlobals` inmutable durante un
/// paso. `resultado` es el paso en curso, expuesto a `asigna`/`precondicion`
/// por la ruta `resultado.*`.
pub struct EntornoMotor {
    locals: HashMap<String, Value>,
    parameters: HashMap<String, Value>,
    file_globals: HashMap<String, Value>,
    resultado: Option<ResultadoStep>,
    parameters_mutables: bool,
}

impl EntornoMotor {
    /// Materializa los `ValorDefinicion` del YAML a `expr::Value` al iniciar
    /// la secuencia. `Parameters` y `FileGlobals` se cargan aquí; `Locals`
    /// parte de sus valores iniciales declarados y muta con `asigna`. Es el
    /// constructor de la **raíz**: `parameters` son de sólo lectura (no hay
    /// llamador al que devolver).
    pub fn desde_definicion(def: &DefinicionSecuencia) -> Self {
        EntornoMotor {
            locals: valor_map(&def.locals),
            parameters: valor_map(&def.parameters),
            file_globals: valor_map(&def.file_globals),
            resultado: None,
            parameters_mutables: false,
        }
    }

    /// Constructor de una **subsecuencia** (M4b): `argumentos` son los
    /// `parameters` inyectados por el sequence call del padre (by-reference:
    /// copia de `locals.X` del padre), y `parameters_mutables = true` para
    /// que la subsecuencia pueda escribir en ellos y devolverlos al padre.
    /// `locals`/`file_globals` se materializan de la definición de la subsec.
    pub fn desde_definicion_con_argumentos(
        def: &DefinicionSecuencia,
        argumentos: HashMap<String, Value>,
        parameters_mutables: bool,
    ) -> Self {
        EntornoMotor {
            locals: valor_map(&def.locals),
            parameters: argumentos,
            file_globals: valor_map(&def.file_globals),
            resultado: None,
            parameters_mutables,
        }
    }

    /// Expone el resultado del paso recién corrido a `asigna` (la ruta
    /// `resultado.*`).
    pub fn set_resultado(&mut self, r: ResultadoStep) {
        self.resultado = Some(r);
    }

    /// Quita el resultado (antes de evaluar la precondición, que no debe ver
    /// el resultado de un paso anterior).
    pub fn limpia_resultado(&mut self) {
        self.resultado = None;
    }

    /// Snapshot de `Locals` (para inspección/depuración/tests).
    pub fn locals(&self) -> &HashMap<String, Value> {
        &self.locals
    }

    /// Snapshot de `Parameters` (para que el motor lea los valores finales
    /// al volver de una subsecuencia y copiarlos al `locals` del padre).
    pub fn parameters(&self) -> &HashMap<String, Value> {
        &self.parameters
    }
}

impl Entorno for EntornoMotor {
    fn lee(&self, scope: Scope, campo: &str) -> Result<Value, ErrorExpr> {
        match scope {
            Scope::Locals => self
                .locals
                .get(campo)
                .cloned()
                .ok_or_else(|| ErrorExpr::entorno(0, format!("no existe 'locals.{campo}'"))),
            Scope::Parameters => self
                .parameters
                .get(campo)
                .cloned()
                .ok_or_else(|| ErrorExpr::entorno(0, format!("no existe 'parameters.{campo}'"))),
            Scope::FileGlobals => self
                .file_globals
                .get(campo)
                .cloned()
                .ok_or_else(|| ErrorExpr::entorno(0, format!("no existe 'file_globals.{campo}'"))),
            Scope::Resultado => Ok(match campo {
                // Laxa: si no hay resultado en curso, todo es Nulo.
                "estado" => self
                    .resultado
                    .as_ref()
                    .map(|r| Value::Texto(r.estado.clone()))
                    .unwrap_or(Value::Nulo),
                "valor_medido" => self
                    .resultado
                    .as_ref()
                    .and_then(|r| r.valor_medido.map(Value::Numero))
                    .unwrap_or(Value::Nulo),
                "mensaje" => self
                    .resultado
                    .as_ref()
                    .map(|r| Value::Texto(r.mensaje.clone()))
                    .unwrap_or(Value::Nulo),
                _ => Value::Nulo,
            }),
        }
    }

    fn escribe(&mut self, scope: Scope, campo: &str, valor: Value) -> Result<(), ErrorExpr> {
        match scope {
            Scope::Locals => {
                self.locals.insert(campo.to_string(), valor);
                Ok(())
            }
            // M4b: una subsecuencia puede escribir en sus `parameters` (canal
            // de retorno by-reference). La raíz no (no hay a quién devolver).
            Scope::Parameters if self.parameters_mutables => {
                self.parameters.insert(campo.to_string(), valor);
                Ok(())
            }
            // `file_globals` nunca se muta; `parameters` en la raíz tampoco;
            // `resultado` nunca se escribe directamente (lo setea el motor).
            Scope::Parameters | Scope::FileGlobals | Scope::Resultado => Err(ErrorExpr::entorno(
                0,
                format!("no se puede escribir en '{}.{campo}' (sólo locals)", scope.nombre()),
            )),
        }
    }
}

/// Convierte un mapa de `ValorDefinicion` (datos del YAML) a `expr::Value`
/// (runtime). Es la materialización que hace `desde_definicion`.
fn valor_map(map: &HashMap<String, ValorDefinicion>) -> HashMap<String, Value> {
    map.iter()
        .map(|(k, v)| (k.clone(), v.a_value()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use expr::{eval, eval_sentencias, Expresion, Sentencia};
    use modelo::{DefinicionSecuencia, ResultadoStep, ValorDefinicion};

    fn secuencia_con(locals: &[(&str, ValorDefinicion)]) -> DefinicionSecuencia {
        let mut def = DefinicionSecuencia { nombre: "t".into(), ..Default::default() };
        def.pasos_main = vec![modelo::DefinicionPaso::nuevo("p", 1)];
        for (k, v) in locals {
            def.locals.insert((*k).to_string(), v.clone());
        }
        def
    }

    #[test]
    fn materializa_locals_de_la_definicion() {
        let def = secuencia_con(&[("x", ValorDefinicion::Numero(0.0)), ("ok", ValorDefinicion::Bool(false))]);
        let env = EntornoMotor::desde_definicion(&def);
        assert_eq!(env.locals().get("x"), Some(&Value::Numero(0.0)));
        assert_eq!(env.locals().get("ok"), Some(&Value::Bool(false)));
    }

    #[test]
    fn file_globals_se_leen_como_value() {
        let mut def = DefinicionSecuencia::default();
        def.file_globals.insert("lote".into(), ValorDefinicion::Texto("A".into()));
        let env = EntornoMotor::desde_definicion(&def);
        let e = expr::parse_expresion("file_globals.lote").unwrap();
        assert_eq!(eval(&e, &env).unwrap(), Value::Texto("A".into()));
    }

    #[test]
    fn leer_local_inexistente_es_error_estricto() {
        let def = secuencia_con(&[]);
        let env = EntornoMotor::desde_definicion(&def);
        let e = expr::parse_expresion("locals.inexistente").unwrap();
        assert!(eval(&e, &env).is_err());
    }

    #[test]
    fn resultado_valor_medido_es_nulo_si_no_hay_resultado() {
        let def = secuencia_con(&[]);
        let env = EntornoMotor::desde_definicion(&def);
        let e = expr::parse_expresion("resultado.valor_medido").unwrap();
        assert_eq!(eval(&e, &env).unwrap(), Value::Nulo);
    }

    #[test]
    fn resultado_estado_y_valor_medido_se_exponen_tras_set() {
        let def = secuencia_con(&[]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::medido_valor("m", "paso", "ok", 4.2));
        assert_eq!(eval(&expr::parse_expresion("resultado.estado").unwrap(), &env).unwrap(), Value::Texto("paso".into()));
        assert_eq!(eval(&expr::parse_expresion("resultado.valor_medido").unwrap(), &env).unwrap(), Value::Numero(4.2));
    }

    #[test]
    fn asigna_escribe_en_locals() {
        let def = secuencia_con(&[("x", ValorDefinicion::Numero(0.0))]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::medido_valor("m", "paso", "ok", 4.2));
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Locals,
            campo: "x".into(),
            valor: expr::parse_expresion("resultado.valor_medido").unwrap(),
        }];
        eval_sentencias(&stmts, &mut env).unwrap();
        assert_eq!(env.locals().get("x"), Some(&Value::Numero(4.2)));
    }

    #[test]
    fn asigna_a_file_globals_es_error() {
        let def = secuencia_con(&[]);
        let mut env = EntornoMotor::desde_definicion(&def);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::FileGlobals,
            campo: "x".into(),
            valor: Expresion::Lit(Value::Numero(1.0)),
        }];
        assert!(eval_sentencias(&stmts, &mut env).is_err());
    }

    // --- M4b: parameters mutables sólo en subsecuencias ---

    /// En la **raíz**, escribir en `parameters` sigue siendo error: no hay
    /// llamador al que devolver (la regla "sólo se muta Locals" se mantiene
    /// para la raíz).
    #[test]
    fn raiz_no_puede_escribir_en_parameters() {
        let mut def = DefinicionSecuencia::default();
        def.parameters.insert("p".into(), ValorDefinicion::Numero(0.0));
        let mut env = EntornoMotor::desde_definicion(&def);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Parameters,
            campo: "p".into(),
            valor: Expresion::Lit(Value::Numero(1.0)),
        }];
        assert!(eval_sentencias(&stmts, &mut env).is_err(), "raíz: parameters es read-only");
        // El valor original sigue ahí (no se mutó).
        assert_eq!(env.parameters().get("p"), Some(&Value::Numero(0.0)));
    }

    /// En una **subsecuencia**, escribir en `parameters` es Ok: es el canal
    /// de retorno by-reference del sequence call (ADR-0010).
    #[test]
    fn subsecuencia_puede_escribir_en_parameters() {
        let mut def = DefinicionSecuencia::default();
        def.parameters.insert("p".into(), ValorDefinicion::Numero(0.0));
        let args = HashMap::from([("p".to_string(), Value::Numero(5.0))]);
        let mut env = EntornoMotor::desde_definicion_con_argumentos(&def, args, true);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Parameters,
            campo: "p".into(),
            valor: Expresion::Lit(Value::Numero(42.0)),
        }];
        eval_sentencias(&stmts, &mut env).expect("subsecuencia: parameters es escribible");
        assert_eq!(env.parameters().get("p"), Some(&Value::Numero(42.0)));
    }

    /// Aunque `parameters` sea mutable en la subsecuencia, `file_globals`
    /// **nunca** se muta (la regla se mantiene para ese scope).
    #[test]
    fn subsecuencia_no_puede_escribir_en_file_globals() {
        let mut def = DefinicionSecuencia::default();
        def.file_globals.insert("g".into(), ValorDefinicion::Numero(0.0));
        let args = HashMap::new();
        let mut env = EntornoMotor::desde_definicion_con_argumentos(&def, args, true);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::FileGlobals,
            campo: "g".into(),
            valor: Expresion::Lit(Value::Numero(1.0)),
        }];
        assert!(eval_sentencias(&stmts, &mut env).is_err(), "file_globals nunca escribible");
    }
}