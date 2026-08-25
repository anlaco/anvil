//! Árbol de sintaxis abstracta del lenguaje de expresiones de anvil.
//!
//! El subconjunto MVP (RF-35) separa **expresiones** (producen un [`Value`],
//! no mutan) de **sentencias** (mutan `Locals`, no devuelven valor). Mezclarlas
//! (todo expresión, estilo Rust/C) añadiría complejidad innecesaria y
//! contradiría el modelo mental Python/Scilab del diseño.

use crate::value::Value;

/// Un alcance de variable. El motor decide cómo resolver cada uno; el engine
/// sólo los nombra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Variables locales de la secuencia en ejecución (mutables por `asigna`).
    Locals,
    /// Parámetros de entrada/salida de una secuencia llamada. En M4-núcleo
    /// (sin sequence call) están vacíos y reservados.
    Parameters,
    /// Globales del archivo de secuencia (compartidas, inmutables durante un
    /// paso).
    FileGlobals,
    /// El resultado del paso en curso (`resultado.estado`,
    /// `resultado.valor_medido`, `resultado.mensaje`). Sólo lectura.
    Resultado,
}

impl Scope {
    /// Nombre textual usado en el lenguaje (`locals`, `parameters`, …).
    pub fn nombre(&self) -> &'static str {
        match self {
            Scope::Locals => "locals",
            Scope::Parameters => "parameters",
            Scope::FileGlobals => "file_globals",
            Scope::Resultado => "result",
        }
    }

    /// Inverso de [`nombre`]: `None` si el texto no es un scope conocido.
    pub fn de_nombre(s: &str) -> Option<Self> {
        match s {
            "locals" => Some(Scope::Locals),
            "parameters" => Some(Scope::Parameters),
            "file_globals" => Some(Scope::FileGlobals),
            "result" => Some(Scope::Resultado),
            _ => None,
        }
    }
}

/// Operador binario. Agrupa aritmética, comparación y lógica; el evaluator
/// valida los tipos por operador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Aritmética (sólo números).
    Add,
    Sub,
    Mul,
    Div,
    // Comparación de orden (sólo números).
    Lt,
    Le,
    Gt,
    Ge,
    // Igualdad (cualquier tipo comparable; tipos distintos → false/true).
    Eq,
    Ne,
    // Lógica (sólo bool, con cortocircuito).
    And,
    Or,
}

/// Operador unario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Negación aritmética (`-x`).
    Neg,
    /// Negación lógica (`not x`).
    Not,
}

/// Una expresión: produce un [`Value`], no muta nada.
#[derive(Debug, Clone, PartialEq)]
pub enum Expresion {
    /// Literal: número, booleano, texto, `nulo`.
    Lit(Value),
    /// Acceso a una variable por scope + campo (`locals.voltaje_leido`,
    /// `resultado.valor_medido`).
    Var { scope: Scope, campo: String },
    /// Operación binaria.
    BinOp {
        op: BinOp,
        izq: Box<Expresion>,
        der: Box<Expresion>,
    },
    /// Operación unaria.
    UnOp { op: UnOp, operando: Box<Expresion> },
}

/// Una sentencia: produce efecto (mutación de `Locals`), no devuelve valor.
/// Hoy sólo hay asignación; post-MVP podría haber `if`/`while` en el flujo.
#[derive(Debug, Clone, PartialEq)]
pub enum Sentencia {
    /// `scope.campo = expresion`. El motor hace valer que `scope` sea `Locals`
    /// (regla "sólo se muta Locals"); escribir en otro scope es error de
    /// evaluación, no de sintaxis.
    Assign {
        scope: Scope,
        campo: String,
        valor: Expresion,
    },
}
