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

/// `result.outputs.tension` llega aquí como `campo == "salidas.tension"`:
/// el parser mete el nombre compuesto dentro de `campo` (ADR-0020). Devuelve
/// el nombre de la salida pedida, o `None` si `campo` no es una salida.
///
/// Se deriva de `expr::CAMPO_SALIDAS` en vez de repetir el literal: el parser
/// y el entorno tienen que estar de acuerdo en cómo se escribe, y dos copias
/// del mismo string es cómo dejan de estarlo.
fn salida_pedida(campo: &str) -> Option<&str> {
    campo
        .strip_prefix(expr::CAMPO_SALIDAS)
        .and_then(|resto| resto.strip_prefix('.'))
}

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
            Scope::FileGlobals => {
                self.file_globals.get(campo).cloned().ok_or_else(|| {
                    ErrorExpr::entorno(0, format!("no existe 'file_globals.{campo}'"))
                })
            }
            Scope::Resultado => match campo {
                // Laxa en el *valor*: si no hay resultado en curso, o si el
                // paso no midió, es Nulo — `valor_medido` puede faltar
                // legítimamente.
                "status" => Ok(self
                    .resultado
                    .as_ref()
                    .map(|r| Value::Texto(r.estado.clone()))
                    .unwrap_or(Value::Nulo)),
                "measured_value" => Ok(self
                    .resultado
                    .as_ref()
                    .and_then(|r| r.valor_medido.map(Value::Numero))
                    .unwrap_or(Value::Nulo)),
                "message" => Ok(self
                    .resultado
                    .as_ref()
                    .map(|r| Value::Texto(r.mensaje.clone()))
                    .unwrap_or(Value::Nulo)),
                // ADR-0020: `result.outputs.<nombre>`. Estricta también
                // aquí, y por el mismo motivo que los otros campos: una
                // salida que el paso no devolvió **no es `Nulo`**, es que la
                // secuencia pide algo que no existe. Devolver `Nulo` lo
                // volcaría a una local y la secuencia saldría verde.
                //
                // La diferencia con los tres campos fijos es *cuándo* se caza:
                // esto no es validable al cargar, porque el cargador no sabe
                // qué devuelve un paso hasta que corre. Es la excepción a la
                // regla de detección de ADR-0019 que el propio ADR-0020 asume,
                // y lo que le devolvería el terreno a `--validate` es la
                // introspección de firma del #45.
                _ if salida_pedida(campo).is_some() => {
                    let quiere = salida_pedida(campo).expect("acabamos de comprobarlo");
                    let r = self.resultado.as_ref().ok_or_else(|| {
                        ErrorExpr::entorno(
                            0,
                            format!("no hay resultado en curso del que leer 'outputs.{quiere}'"),
                        )
                    })?;
                    r.salidas
                        .iter()
                        .find(|(n, _)| n == quiere)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            let trajo = if r.salidas.is_empty() {
                                "el paso no devolvió ninguna salida".to_string()
                            } else {
                                format!(
                                    "el paso devolvió {}",
                                    r.salidas
                                        .iter()
                                        .map(|(n, _)| format!("'{n}'"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            };
                            ErrorExpr::entorno(
                                0,
                                format!("no existe 'result.outputs.{quiere}': {trajo}"),
                            )
                        })
                }
                // Estricta en el *nombre*: los campos son tres y conocidos
                // (`modelo::CAMPOS_RESULTADO`), así que `result.measured_valu`
                // no es un dato ausente, es un typo. Devolvía `Nulo`, y ese
                // `Nulo` se volcaba a una local y la secuencia salía verde
                // (ADR-0019, Regla 2, issue #27). Ahora falla la asigna, y una
                // asigna que falla convierte el paso en `error`.
                //
                // El cargador lo caza antes, al validar (`--validate`); esto es
                // la red de debajo, para el camino que no pase por él.
                _ => Err(ErrorExpr::entorno(
                    0,
                    format!(
                        "no existe 'result.{campo}': los campos de 'result' son {}",
                        modelo::CAMPOS_RESULTADO
                            .map(|c| format!("'{c}'"))
                            .join(", ")
                    ),
                )),
            },
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
                format!(
                    "no se puede escribir en '{}.{campo}' (sólo locals)",
                    scope.nombre()
                ),
            )),
        }
    }
}

/// Convierte un mapa de `ValorDefinicion` (datos del YAML) a `expr::Value`
/// (runtime). Es la materialización que hace `desde_definicion`.
fn valor_map(map: &HashMap<String, ValorDefinicion>) -> HashMap<String, Value> {
    map.iter().map(|(k, v)| (k.clone(), v.a_value())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use expr::{eval, eval_sentencias, Expresion, Sentencia};
    use modelo::{DefinicionSecuencia, ResultadoStep, ValorDefinicion};

    fn secuencia_con(locals: &[(&str, ValorDefinicion)]) -> DefinicionSecuencia {
        let mut def = DefinicionSecuencia {
            nombre: "t".into(),
            ..Default::default()
        };
        def.pasos_main = vec![modelo::DefinicionPaso::nuevo("p", 1)];
        for (k, v) in locals {
            def.locals.insert((*k).to_string(), v.clone());
        }
        def
    }

    #[test]
    fn materializa_locals_de_la_definicion() {
        let def = secuencia_con(&[
            ("x", ValorDefinicion::Numero(0.0)),
            ("ok", ValorDefinicion::Bool(false)),
        ]);
        let env = EntornoMotor::desde_definicion(&def);
        assert_eq!(env.locals().get("x"), Some(&Value::Numero(0.0)));
        assert_eq!(env.locals().get("ok"), Some(&Value::Bool(false)));
    }

    #[test]
    fn file_globals_se_leen_como_value() {
        let mut def = DefinicionSecuencia::default();
        def.file_globals
            .insert("lote".into(), ValorDefinicion::Texto("A".into()));
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
        let e = expr::parse_expresion("result.measured_value").unwrap();
        assert_eq!(eval(&e, &env).unwrap(), Value::Nulo);
    }

    #[test]
    fn resultado_estado_y_valor_medido_se_exponen_tras_set() {
        let def = secuencia_con(&[]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::medido_valor("m", "paso", "ok", 4.2));
        assert_eq!(
            eval(&expr::parse_expresion("result.status").unwrap(), &env).unwrap(),
            Value::Texto("paso".into())
        );
        assert_eq!(
            eval(
                &expr::parse_expresion("result.measured_value").unwrap(),
                &env
            )
            .unwrap(),
            Value::Numero(4.2)
        );
    }

    /// ADR-0019, Regla 2 (issue #27): el nombre del campo es **estricto**. Un
    /// `result.measured_valu` valía `nothing`, y ese `nothing` se volcaba a
    /// la local que decidía el veredicto.
    #[test]
    fn leer_un_campo_inexistente_de_resultado_es_error() {
        let def = secuencia_con(&[]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::medido_valor("m", "paso", "ok", 4.2));
        let e = expr::parse_expresion("result.measured_valu").unwrap();
        let err = eval(&e, &env).expect_err("un typo no es un dato ausente");
        let msg = err.to_string();
        assert!(msg.contains("measured_valu"), "nombra el campo: {msg}");
        assert!(msg.contains("'measured_value'"), "y los válidos: {msg}");
    }

    /// Lo estricto es el **nombre**, no el valor: los tres campos conocidos se
    /// leen sin error aunque el paso no haya medido (`valor_medido` puede
    /// faltar legítimamente, y entonces vale `nothing`).
    #[test]
    fn los_tres_campos_de_resultado_se_leen_siempre() {
        let def = secuencia_con(&[]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::nuevo("m", "paso", "sin medida"));
        for campo in modelo::CAMPOS_RESULTADO {
            let e = expr::parse_expresion(&format!("result.{campo}")).unwrap();
            eval(&e, &env).unwrap_or_else(|_| panic!("'resultado.{campo}' debe ser legible"));
        }
        // Y sin resultado en curso, tampoco fallan: valen `nothing`.
        env.limpia_resultado();
        let e = expr::parse_expresion("result.measured_value").unwrap();
        assert_eq!(eval(&e, &env).unwrap(), Value::Nulo);
    }

    #[test]
    fn asigna_escribe_en_locals() {
        let def = secuencia_con(&[("x", ValorDefinicion::Numero(0.0))]);
        let mut env = EntornoMotor::desde_definicion(&def);
        env.set_resultado(ResultadoStep::medido_valor("m", "paso", "ok", 4.2));
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Locals,
            campo: "x".into(),
            valor: expr::parse_expresion("result.measured_value").unwrap(),
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
        def.parameters
            .insert("p".into(), ValorDefinicion::Numero(0.0));
        let mut env = EntornoMotor::desde_definicion(&def);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::Parameters,
            campo: "p".into(),
            valor: Expresion::Lit(Value::Numero(1.0)),
        }];
        assert!(
            eval_sentencias(&stmts, &mut env).is_err(),
            "raíz: parameters es read-only"
        );
        // El valor original sigue ahí (no se mutó).
        assert_eq!(env.parameters().get("p"), Some(&Value::Numero(0.0)));
    }

    /// En una **subsecuencia**, escribir en `parameters` es Ok: es el canal
    /// de retorno by-reference del sequence call (ADR-0010).
    #[test]
    fn subsecuencia_puede_escribir_en_parameters() {
        let mut def = DefinicionSecuencia::default();
        def.parameters
            .insert("p".into(), ValorDefinicion::Numero(0.0));
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
        def.file_globals
            .insert("g".into(), ValorDefinicion::Numero(0.0));
        let args = HashMap::new();
        let mut env = EntornoMotor::desde_definicion_con_argumentos(&def, args, true);
        let stmts = vec![Sentencia::Assign {
            scope: Scope::FileGlobals,
            campo: "g".into(),
            valor: Expresion::Lit(Value::Numero(1.0)),
        }];
        assert!(
            eval_sentencias(&stmts, &mut env).is_err(),
            "file_globals nunca escribible"
        );
    }
}
