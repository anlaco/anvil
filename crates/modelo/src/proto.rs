//! The messages of `paso.proto`, hand-declared with `prost` (wasi-grpc v0.1
//! ships no codegen). The `.proto` is the source of truth for the contract:
//! touch one and you must touch the other.
//!
//! The three measurement fields travel as `string` because that is how the
//! contract defined them. In proto3 an empty `string` is not transmitted.

use prost::Message;

/// The gRPC method path. No `package` in the `.proto`, so it is plainly
/// `/<service>/<method>`.
pub const ROUTE_INVOKE: &str = "/StepExecutor/Invoke";

/// The path of the catalog RPC (ADR-0021). An executor is allowed not to
/// serve it: the engine then leaves its steps unchecked and says so.
pub const ROUTE_DESCRIBE: &str = "/StepExecutor/Describe";

/// The contract version this binary speaks (ADR-0020 §4).
///
/// - **1** = the original contract: `StepRequest{name, attempt}`, no inputs
///   and no outputs. An executor speaking contract 1 does not know the tag
///   and returns `0` by proto3 default, which is what gives it away.
/// - **2** = named inputs and outputs.
/// - **3** = the English contract (state vocabulary, field names).
///
/// Raise it for every change where **an old peer's silence could alter a
/// verdict**. What a peer can ignore without changing the claim made about
/// the unit (an informative field, a trace) does not raise it — which is why
/// `Describe` (ADR-0021) did *not*: an executor that declines to describe
/// itself measures exactly the same thing.
pub const CONTRACT: i32 = 3;

/// A named, typed value, exactly as it travels on the wire.
///
/// The `oneof` is an `Option` because proto3 allows no branch to be set — and
/// that is precisely what has to be detectable: **a `oneof` with no branch is
/// an error, not a zero** (ADR-0020 §2). See `Value::a_value`.
#[derive(Clone, PartialEq, Message)]
pub struct Value {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(oneof = "value::Dato", tags = "2, 3, 4")]
    pub dato: Option<value::Dato>,
}

pub mod value {
    /// The three branches of the `oneof`, in the same order as the `.proto`.
    /// They are the three `expr::Value` variants that carry a value (all but
    /// `Nulo`).
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Dato {
        #[prost(double, tag = "2")]
        Numero(f64),
        #[prost(string, tag = "3")]
        Texto(String),
        #[prost(bool, tag = "4")]
        Booleano(bool),
    }
}

impl Value {
    /// Builds a wire `Value` from an already-evaluated `expr::Value`.
    ///
    /// `Value::Nulo` **has no representation**: this returns `None`. A null is
    /// not sent as an empty `oneof` — that is exactly what the receiver must be
    /// able to reject — so the caller decides what to do with the absence.
    pub fn desde_value(nombre: &str, v: &expr::Value) -> Option<Value> {
        let dato = match v {
            expr::Value::Numero(x) => value::Dato::Numero(*x),
            expr::Value::Texto(s) => value::Dato::Texto(s.clone()),
            expr::Value::Bool(b) => value::Dato::Booleano(*b),
            expr::Value::Nulo => return None,
        };
        Some(Value {
            name: nombre.to_string(),
            dato: Some(dato),
        })
    }

    /// The `expr::Value` it stands for, or `None` if the `oneof` arrived empty.
    ///
    /// Returning `None` and not `Value::Nulo` is deliberate: they are different
    /// things. `Nulo` is a known absent value; this is a message that does not
    /// say what type it is, and the receiver must treat it as an error.
    pub fn a_value(&self) -> Option<expr::Value> {
        match self.dato.as_ref()? {
            value::Dato::Numero(x) => Some(expr::Value::Numero(*x)),
            value::Dato::Texto(s) => Some(expr::Value::Texto(s.clone())),
            value::Dato::Booleano(b) => Some(expr::Value::Bool(*b)),
        }
    }

    /// The wire type of this value, or [`ValueType::Unspecified`] if the
    /// `oneof` arrived empty. Used to check a declared parameter against the
    /// type the executor says it takes (ADR-0021 §5).
    pub fn value_type(&self) -> ValueType {
        match self.dato.as_ref() {
            None => ValueType::Unspecified,
            Some(value::Dato::Numero(_)) => ValueType::Number,
            Some(value::Dato::Texto(_)) => ValueType::Text,
            Some(value::Dato::Booleano(_)) => ValueType::Boolean,
        }
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct StepRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    /// Attempt number, starting at 1. Steps receive it so they can simulate
    /// transient failures (see `pasos_demo`).
    #[prost(int32, tag = "2")]
    pub attempt: i32,
    /// This invocation's inputs, already evaluated (ADR-0020 §1).
    #[prost(message, repeated, tag = "3")]
    pub inputs: Vec<Value>,
    /// The contract version the engine speaks. See [`CONTRACT`].
    #[prost(int32, tag = "4")]
    pub contract: i32,
}

/// The type of a described parameter or output (ADR-0021 §5). Exactly the
/// three of [`Value`]: this describes the wire, it is not a type system.
///
/// `Unspecified` is the proto3 default, so **silence lands here** — an
/// executor that names a parameter without saying its type leaves it
/// unchecked, and the report says so. It is never read as "number".
#[derive(Clone, Copy, Debug, PartialEq, Eq, ::prost::Enumeration)]
#[repr(i32)]
pub enum ValueType {
    Unspecified = 0,
    Number = 1,
    Text = 2,
    Boolean = 3,
}

impl ValueType {
    /// The name used in diagnostics — the same three words the `.proto` uses,
    /// so a message about a type mismatch reads the way the contract is
    /// written.
    pub fn name(&self) -> &'static str {
        match self {
            ValueType::Unspecified => "unspecified",
            ValueType::Number => "number",
            ValueType::Text => "text",
            ValueType::Boolean => "boolean",
        }
    }
}

/// One input a step accepts, as its executor describes it.
#[derive(Clone, PartialEq, Message)]
pub struct ParameterSpec {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(enumeration = "ValueType", tag = "2")]
    pub r#type: i32,
    /// Whether the step needs it in order to measure. A missing optional is
    /// the step's business; a missing required one is a sequence Anvil can
    /// reject before touching the unit.
    #[prost(bool, tag = "3")]
    pub required: bool,
    /// The value the step applies when an optional input is not sent. Purely
    /// informative: the engine sends nothing: a default applied in two places
    /// is a default that drifts.
    #[prost(message, optional, tag = "4")]
    pub default: Option<Value>,
    #[prost(string, tag = "5")]
    pub doc: String,
}

impl ParameterSpec {
    /// The declared type, decoded. An unknown number on the wire reads as
    /// [`ValueType::Unspecified`] — unchecked, never guessed.
    pub fn value_type(&self) -> ValueType {
        ValueType::try_from(self.r#type).unwrap_or(ValueType::Unspecified)
    }

    /// An input the step needs in order to measure.
    pub fn required(name: &str, r#type: ValueType) -> Self {
        ParameterSpec {
            name: name.to_string(),
            r#type: r#type as i32,
            required: true,
            default: None,
            doc: String::new(),
        }
    }

    /// An input the step can do without, and what it uses instead. The default
    /// is declared for the reader and for the editor, **not** for the engine:
    /// applying it in two places is how a default drifts.
    pub fn optional(name: &str, r#type: ValueType, default: expr::Value) -> Self {
        ParameterSpec {
            name: name.to_string(),
            r#type: r#type as i32,
            required: false,
            default: Value::desde_value(name, &default),
            doc: String::new(),
        }
    }

    /// The one line a human reads: what it means, and its unit if it has one.
    pub fn con_doc(mut self, doc: &str) -> Self {
        self.doc = doc.to_string();
        self
    }
}

impl OutputSpec {
    /// A named value the step returns besides its measurement.
    pub fn nueva(name: &str, r#type: ValueType, doc: &str) -> Self {
        OutputSpec {
            name: name.to_string(),
            r#type: r#type as i32,
            doc: doc.to_string(),
        }
    }
}

/// One named output a step returns besides its measurement. Describing these
/// is what lets `assign: result.outputs.<name>` be checked without running —
/// the one declared exception to the detection rule of ADR-0019 (ADR-0020 §3).
#[derive(Clone, PartialEq, Message)]
pub struct OutputSpec {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(enumeration = "ValueType", tag = "2")]
    pub r#type: i32,
    #[prost(string, tag = "3")]
    pub doc: String,
}

/// The signature of one step: what it is called, what it takes, what it gives.
#[derive(Clone, PartialEq, Message)]
pub struct StepSpec {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(message, repeated, tag = "2")]
    pub inputs: Vec<ParameterSpec>,
    #[prost(message, repeated, tag = "3")]
    pub outputs: Vec<OutputSpec>,
    #[prost(string, tag = "4")]
    pub doc: String,
}

impl StepSpec {
    /// The spec of one input by name, or `None` if the step does not declare
    /// it — which is what makes a typo in the sequence a load-time finding.
    pub fn input(&self, name: &str) -> Option<&ParameterSpec> {
        self.inputs.iter().find(|p| p.name == name)
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct CatalogRequest {
    /// The contract version the engine speaks. See [`CONTRACT`].
    #[prost(int32, tag = "1")]
    pub contract: i32,
}

/// What an executor answers about itself (ADR-0021).
#[derive(Clone, PartialEq, Message)]
pub struct Catalog {
    #[prost(message, repeated, tag = "1")]
    pub steps: Vec<StepSpec>,
    /// **The whole point of this field.** An empty `steps` is ambiguous — "I
    /// serve nothing" or "I do not describe myself"? proto3 makes `false` the
    /// default, so an executor that stays silent (an empty body, an old peer,
    /// an `UNIMPLEMENTED` that arrives as a broken stream) says *"do not check
    /// me"*, which is the only safe reading. An executor that positively
    /// serves nothing sets it to `true`.
    #[prost(bool, tag = "2")]
    pub describes: bool,
    /// Echo of the contract version the executor understood, as in
    /// [`StepResult::contract`].
    #[prost(int32, tag = "3")]
    pub contract: i32,
}

impl Catalog {
    /// The catalog of an executor that does describe itself.
    pub fn descrito(steps: Vec<StepSpec>) -> Self {
        Catalog {
            steps,
            describes: true,
            contract: CONTRACT,
        }
    }

    /// The spec of one step by name, or `None` if this executor does not serve
    /// it. Only meaningful when `describes` is `true`.
    pub fn step(&self, name: &str) -> Option<&StepSpec> {
        self.steps.iter().find(|s| s.name == name)
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct StepResult {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub status: String,
    #[prost(string, tag = "3")]
    pub message: String,
    #[prost(string, tag = "4")]
    pub measured_value: String,
    #[prost(string, tag = "5")]
    pub limit_min: String,
    #[prost(string, tag = "6")]
    pub limit_max: String,
    /// Named values the step returns besides the measurement. **They take no
    /// part in the verdict** (ADR-0008: the engine judges `measured_value`
    /// against the sequence's `limit`, and nothing else).
    #[prost(message, repeated, tag = "7")]
    pub outputs: Vec<Value>,
    /// The echo: the contract version the executor actually understood. See
    /// [`CONTRACT`] and ADR-0020 §4b.
    #[prost(int32, tag = "8")]
    pub contract: i32,
}

/// An optional `f64` to the text that travels on the wire: empty when there
/// is no value. Integers are written without decimals ("5", not "5.0").
///
/// It is `pub` so the report sinks (CSV) reuse the wire's number format
/// instead of reimplementing it: one source of truth for how a measurement
/// is written.
pub fn a_texto(v: Option<f64>) -> String {
    match v {
        None => String::new(),
        Some(x) if x.fract() == 0.0 => format!("{}", x as i64),
        Some(x) => format!("{x}"),
    }
}

fn de_texto(s: &str) -> Option<f64> {
    if s.is_empty() {
        None
    } else {
        s.parse().ok()
    }
}

impl From<&crate::ResultadoStep> for StepResult {
    fn from(r: &crate::ResultadoStep) -> Self {
        StepResult {
            name: r.nombre.clone(),
            status: r.estado.clone(),
            message: r.mensaje.clone(),
            measured_value: a_texto(r.valor_medido),
            limit_min: a_texto(r.limite_min),
            limit_max: a_texto(r.limite_max),
            // A `Value::Nulo` has no wire representation and is dropped:
            // sending it as an empty `oneof` would be sending exactly what the
            // receiver must reject. A null output is an output not returned.
            outputs: r
                .salidas
                .iter()
                .filter_map(|(n, v)| Value::desde_value(n, v))
                .collect(),
            contract: CONTRACT,
        }
    }
}

/// An output off the wire that Anvil cannot interpret.
///
/// It is a `Result` and not a silent discard on purpose: a `oneof` with no
/// branch does not say what type the value is, and swallowing it would be
/// inventing a fact about the unit under test. Rule 2 of ADR-0019 — what is
/// not understood is `error`, never `fail` and never `pass`.
#[derive(Debug, Clone, PartialEq)]
pub struct SalidaSinTipo {
    /// The name the output carried (empty if it carried none either).
    pub nombre: String,
}

impl std::fmt::Display for SalidaSinTipo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quien = if self.nombre.is_empty() {
            "una salida sin nombre".to_string()
        } else {
            format!("la salida '{}'", self.nombre)
        };
        write!(
            f,
            "{quien} llegó sin tipo: el ejecutor no puso ninguna de las tres \
             ramas (numero, texto, booleano)"
        )
    }
}

impl std::error::Error for SalidaSinTipo {}

impl StepResult {
    /// Translates the wire message into the model, **validating the outputs**.
    ///
    /// It replaces the `From` that was here: an infallible conversion cannot
    /// express that an output arrived untyped, and that case cannot be
    /// discarded in silence.
    pub fn a_resultado(self) -> Result<crate::ResultadoStep, SalidaSinTipo> {
        let mut salidas = Vec::with_capacity(self.outputs.len());
        for v in &self.outputs {
            match v.a_value() {
                Some(valor) => salidas.push((v.name.clone(), valor)),
                None => {
                    return Err(SalidaSinTipo {
                        nombre: v.name.clone(),
                    })
                }
            }
        }
        let mut r = crate::ResultadoStep::from(self);
        r.salidas = salidas;
        Ok(r)
    }
}

impl From<StepResult> for crate::ResultadoStep {
    /// Conversion **without** the outputs: `a_resultado` fills those in, and
    /// it is the one that can fail. Do not use this to read off the wire.
    fn from(p: StepResult) -> Self {
        // `valor_esperado` and `operador` do **not** come off the wire: the
        // contract carries no limits (ADR-0008). They arrive `None` and the
        // engine fills them from the YAML `limit` after the invocation.
        crate::ResultadoStep {
            nombre: p.name,
            estado: p.status,
            mensaje: p.message,
            valor_medido: de_texto(&p.measured_value),
            limite_min: de_texto(&p.limit_min),
            limite_max: de_texto(&p.limit_max),
            valor_esperado: None,
            operador: None,
            // `sub_pasos` does not travel on the wire: sequence call is
            // engine-side (ADR-0010). It arrives `None` and the engine fills
            // it when nesting the subsequence.
            sub_pasos: None,
            // The phase does not travel either: a step does not know which
            // one it runs in. The engine stamps it on receiving the result,
            // before emitting it to the sink.
            fase: crate::Fase::default(),
            // The engine stamps the inputs: it is the one that knows what it
            // sent.
            parametros: Vec::new(),
            // `a_resultado` fills the outputs in; it validates the `oneof`s.
            salidas: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResultadoStep;

    #[test]
    fn round_trip_with_a_measurement() {
        let r = ResultadoStep::medido("medir_voltaje", "fail", "fuera", 4.2, 4.5, 5.5);
        let p: StepResult = (&r).into();
        assert_eq!(p.measured_value, "4.2");
        assert_eq!(p.limit_min, "4.5");
        assert_eq!(p.limit_max, "5.5");
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn round_trip_without_a_measurement() {
        let r = ResultadoStep::nuevo("verificar_led", "pass", "led encendido");
        let p: StepResult = (&r).into();
        assert!(p.measured_value.is_empty());
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn empty_fields_do_not_travel() {
        // proto3: an empty string is not serialized, so a result with no
        // measurement travels with the first three fields only.
        let r = ResultadoStep::nuevo("x", "pass", "ok");
        let p: StepResult = (&r).into();
        let bytes = p.encode_to_vec();
        let redecodificado = StepResult::decode(&bytes[..]).unwrap();
        assert_eq!(redecodificado, p);
        // name + status + message and nothing else.
        assert!(
            !bytes.windows(1).any(|w| w[0] == 0x22),
            "there must be no tag 4"
        );
    }

    #[test]
    fn a_value_round_trips_through_all_three_types() {
        for v in [
            expr::Value::Numero(4.2),
            expr::Value::Texto("banco-3".into()),
            expr::Value::Bool(true),
        ] {
            let cable = Value::desde_value("p", &v).expect("all three types travel");
            let bytes = cable.encode_to_vec();
            let vuelta = Value::decode(&bytes[..]).unwrap();
            assert_eq!(vuelta.name, "p");
            assert_eq!(vuelta.a_value(), Some(v));
        }
    }

    #[test]
    fn a_null_does_not_travel() {
        // It has no wire representation, and sending it as an empty `oneof`
        // would be sending exactly what the receiver has to reject.
        assert_eq!(Value::desde_value("p", &expr::Value::Nulo), None);
    }

    #[test]
    fn an_untyped_output_is_an_error_and_not_a_zero() {
        // Rule 2 of ADR-0019 on the return wire: a `oneof` with no branch does
        // not say what type the value is. Swallowing it would be inventing a
        // fact about the unit under test.
        let p = StepResult {
            name: "medir".into(),
            status: "pass".into(),
            outputs: vec![Value {
                name: "tension".into(),
                dato: None,
            }],
            ..Default::default()
        };
        let e = p.a_resultado().expect_err("an empty oneof cannot pass");
        assert_eq!(e.nombre, "tension");
        assert!(
            e.to_string().contains("tension"),
            "the error names the output"
        );
    }

    #[test]
    fn outputs_reach_the_model_with_their_type() {
        let p = StepResult {
            name: "medir".into(),
            status: "pass".into(),
            outputs: vec![
                Value::desde_value("serie", &expr::Value::Texto("A7".into())).unwrap(),
                Value::desde_value("temp", &expr::Value::Numero(21.5)).unwrap(),
            ],
            ..Default::default()
        };
        let r = p.a_resultado().unwrap();
        assert_eq!(
            r.salidas,
            vec![
                ("serie".to_string(), expr::Value::Texto("A7".into())),
                ("temp".to_string(), expr::Value::Numero(21.5)),
            ]
        );
    }

    #[test]
    fn a_contract_1_executor_returns_a_zero_echo() {
        // What gives an old peer away: it does not know tag 8, so proto3
        // leaves it at the default. It is the basis of the echo check the
        // engine performs (ADR-0020 §4b).
        let viejo = StepResult {
            name: "verificar_led".into(),
            status: "pass".into(),
            message: "led encendido".into(),
            ..Default::default()
        };
        let bytes = viejo.encode_to_vec();
        let eco = StepResult::decode(&bytes[..]).unwrap().contract;
        assert_eq!(eco, 0, "proto3's default is what gives the old peer away");
        assert!(
            eco < CONTRACT,
            "and it has to sit below today's contract, or the engine could \
             not tell it apart"
        );
    }

    #[test]
    fn fields_1_to_6_have_not_moved() {
        // The ADR says they are not touched, and desynchronising tags across
        // the four copies of the contract stops being a compile error and
        // becomes an echo that lies.
        let r = ResultadoStep::medido("m", "pass", "ok", 4.2, 4.5, 5.5);
        let p: StepResult = (&r).into();
        let bytes = p.encode_to_vec();
        let vuelta = StepResult::decode(&bytes[..]).unwrap();
        assert_eq!(vuelta.measured_value, "4.2");
        assert_eq!(vuelta.limit_min, "4.5");
        assert_eq!(vuelta.limit_max, "5.5");
        assert_eq!(vuelta.contract, CONTRACT);
    }

    #[test]
    fn an_integer_has_no_decimals() {
        assert_eq!(a_texto(Some(5.0)), "5");
        assert_eq!(a_texto(Some(4.2)), "4.2");
        assert_eq!(a_texto(None), "");
    }

    /// ADR-0021, the whole reason `describes` exists: an executor that says
    /// nothing —an empty body, an old peer, an `UNIMPLEMENTED` that decodes to
    /// nothing— must read as "do not check me", never as "I serve no steps".
    /// The difference matters: the second reading would make every step of the
    /// sequence a finding.
    #[test]
    fn silence_is_not_an_empty_catalog() {
        let mudo = Catalog::decode(&[][..]).expect("an empty body decodes");
        assert!(!mudo.describes, "silence does not describe");
        assert_eq!(mudo.contract, 0, "and it does not echo the contract either");

        let vacio = Catalog::descrito(Vec::new());
        assert!(vacio.describes, "serving nothing is a positive statement");
        assert_eq!(vacio.contract, CONTRACT);
    }

    #[test]
    fn a_catalog_round_trips_with_its_signatures() {
        let cat = Catalog::descrito(vec![StepSpec {
            name: "measure".into(),
            inputs: vec![ParameterSpec {
                name: "channel".into(),
                r#type: ValueType::Number as i32,
                required: false,
                default: Value::desde_value("channel", &expr::Value::Numero(1.0)),
                doc: "which channel is measured".into(),
            }],
            outputs: vec![OutputSpec {
                name: "temperature".into(),
                r#type: ValueType::Number as i32,
                doc: String::new(),
            }],
            doc: String::new(),
        }]);
        let vuelta = Catalog::decode(&cat.encode_to_vec()[..]).unwrap();
        assert_eq!(vuelta, cat);

        let paso = vuelta.step("measure").expect("the step is in the catalog");
        let canal = paso.input("channel").expect("and so is its input");
        assert_eq!(canal.value_type(), ValueType::Number);
        assert!(!canal.required);
        assert!(
            vuelta.step("measur").is_none(),
            "a typo is not in the catalog"
        );
    }

    /// A type an executor does not state is **unchecked**, not a number. It is
    /// the same rule as the empty `oneof`, applied to the description.
    #[test]
    fn an_unstated_type_reads_as_unspecified() {
        let sin_tipo = ParameterSpec {
            name: "channel".into(),
            ..Default::default()
        };
        assert_eq!(sin_tipo.value_type(), ValueType::Unspecified);
        // And a number nobody has defined yet does not become one of the three
        // either: a future contract's type is unchecked here, not guessed.
        let futuro = ParameterSpec {
            name: "channel".into(),
            r#type: 99,
            ..Default::default()
        };
        assert_eq!(futuro.value_type(), ValueType::Unspecified);
    }

    #[test]
    fn the_wire_type_of_a_value_is_its_branch() {
        for (v, t) in [
            (expr::Value::Numero(1.0), ValueType::Number),
            (expr::Value::Texto("a".into()), ValueType::Text),
            (expr::Value::Bool(true), ValueType::Boolean),
        ] {
            assert_eq!(Value::desde_value("p", &v).unwrap().value_type(), t);
        }
        assert_eq!(
            Value {
                name: "p".into(),
                dato: None
            }
            .value_type(),
            ValueType::Unspecified
        );
    }
}
