//! Los mensajes de `paso.proto`, declarados a mano con `prost` (v0.1 de
//! wasi-grpc no trae codegen). El `.proto` es la fuente de verdad del
//! contrato: si se toca uno, hay que tocar el otro.
//!
//! Los tres campos de medida van como `string` porque así los definió el
//! contrato. En proto3 un `string` vacío no se transmite.

use prost::Message;

/// La ruta del método gRPC. Sin `package` en el `.proto`, así que es
/// directamente `/<servicio>/<método>`.
pub const RUTA_INVOCA: &str = "/StepExecutor/Invoke";

/// La versión de contrato que habla este binario (ADR-0020 §4).
///
/// - **1** = el contrato original: `PeticionPaso{nombre, intento}`, sin
///   parámetros ni salidas. Un ejecutor de contrato 1 no conoce el tag y
///   devuelve `0` por el default de proto3, que es lo que lo delata.
/// - **2** = éste: parámetros de entrada y salidas con nombre.
///
/// Sube todo cambio en el que **el silencio de un par antiguo pueda alterar un
/// veredicto**. Lo que un par puede ignorar sin que la afirmación sobre la
/// unidad cambie (un campo informativo, una traza), no lo sube.
pub const CONTRATO: i32 = 3;

/// Un valor con nombre y tipo, tal y como viaja por el cable.
///
/// El `oneof` es `Option` porque proto3 permite que no venga ninguna rama —
/// y eso es exactamente lo que hay que poder detectar: **un `oneof` sin rama
/// es error, no un cero** (ADR-0020 §2). Ver `Value::a_value`.
#[derive(Clone, PartialEq, Message)]
pub struct Value {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(oneof = "value::Dato", tags = "2, 3, 4")]
    pub dato: Option<value::Dato>,
}

pub mod value {
    /// Las tres ramas del `oneof`, en el mismo orden que el `.proto`. Son los
    /// tres tipos de `expr::Value` que tienen valor (todos menos `Nulo`).
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
    /// Construye un `Value` de cable desde un `expr::Value` ya evaluado.
    ///
    /// `Value::Nulo` **no tiene representación**: devuelve `None`. Un nulo no
    /// se manda como `oneof` vacío —eso es justo lo que el receptor tiene que
    /// poder rechazar— así que quien llame decide qué hacer con la ausencia.
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

    /// El `expr::Value` que representa, o `None` si el `oneof` llegó vacío.
    ///
    /// Devolver `None` y no `Value::Nulo` es deliberado: son cosas distintas.
    /// `Nulo` es un valor ausente conocido; esto es un mensaje que no dice de
    /// qué tipo es, y el receptor tiene que tratarlo como error.
    pub fn a_value(&self) -> Option<expr::Value> {
        match self.dato.as_ref()? {
            value::Dato::Numero(x) => Some(expr::Value::Numero(*x)),
            value::Dato::Texto(s) => Some(expr::Value::Texto(s.clone())),
            value::Dato::Booleano(b) => Some(expr::Value::Bool(*b)),
        }
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct StepRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    /// Número de intento, empezando en 1. Los pasos lo reciben para poder
    /// simular fallos transitorios (ver `pasos_demo`).
    #[prost(int32, tag = "2")]
    pub attempt: i32,
    /// Los parámetros de esta invocación, ya evaluados (ADR-0020 §1).
    #[prost(message, repeated, tag = "3")]
    pub inputs: Vec<Value>,
    /// La versión de contrato que habla el motor. Ver [`CONTRATO`].
    #[prost(int32, tag = "4")]
    pub contract: i32,
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
    /// Valores con nombre que devuelve el paso además de la medida. **No
    /// participan en el veredicto** (ADR-0008: el motor juzga `valor_medido`
    /// contra el `limite` de la secuencia, y nada más).
    #[prost(message, repeated, tag = "7")]
    pub outputs: Vec<Value>,
    /// El eco: la versión de contrato que el ejecutor ha entendido. Ver
    /// [`CONTRATO`] y ADR-0020 §4b.
    #[prost(int32, tag = "8")]
    pub contract: i32,
}

/// Un `f64` opcional al texto que viaja por el cable: vacío si no hay
/// valor. Los enteros se escriben sin decimales ("5" y no "5.0").
///
/// Es `pub` para que los sinks de reporte (CSV) reutilicen el mismo
/// formato numérico del cable y no lo reimplementen: fuente única de
/// verdad para cómo se escribe un número de medida.
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
            // Un `Value::Nulo` no tiene representación en el cable y se
            // descarta: mandarlo como `oneof` vacío sería mandar justo lo que
            // el receptor debe rechazar. Una salida nula es una salida que no
            // se devuelve.
            outputs: r
                .salidas
                .iter()
                .filter_map(|(n, v)| Value::desde_value(n, v))
                .collect(),
            contract: CONTRATO,
        }
    }
}

/// Una salida del cable que Anvil no puede interpretar.
///
/// Es un `Result` y no un descarte silencioso a propósito: un `oneof` sin
/// rama no dice de qué tipo es el valor, y tragárselo sería inventarse un
/// dato de la unidad bajo test. Regla 2 de ADR-0019 — lo que no se entiende
/// es `error`, nunca `fallo` ni `paso`.
#[derive(Debug, Clone, PartialEq)]
pub struct SalidaSinTipo {
    /// El nombre que traía la salida (vacío si tampoco traía nombre).
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
    /// Traduce el mensaje del cable al modelo, **validando las salidas**.
    ///
    /// Sustituye al `From` que había: una conversión infalible no puede
    /// expresar que una salida llegue sin tipo, y ese caso no se puede
    /// descartar en silencio.
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
    /// Conversión **sin** las salidas: las rellena `a_resultado`, que es la
    /// que puede fallar. No la uses directamente para leer del cable.
    fn from(p: StepResult) -> Self {
        // `valor_esperado` y `operador` **no** vienen del cable: el contrato no
        // lleva límites (ADR-0008). Llegan `None` y los rellena el motor desde
        // el `Limite` del YAML tras la invocación.
        crate::ResultadoStep {
            nombre: p.name,
            estado: p.status,
            mensaje: p.message,
            valor_medido: de_texto(&p.measured_value),
            limite_min: de_texto(&p.limit_min),
            limite_max: de_texto(&p.limit_max),
            valor_esperado: None,
            operador: None,
            // `sub_pasos` no viaja en el cable: sequence call es motor-side
            // (ADR-0010). Llega `None` y el motor lo rellena al anidar la
            // subsecuencia.
            sub_pasos: None,
            // La fase tampoco viaja: el paso no sabe en cuál corre. La sella
            // el motor al recibir el resultado, antes de emitirlo al sink.
            fase: crate::Fase::default(),
            // Los parámetros los sella el motor: es él quien sabe qué envió.
            parametros: Vec::new(),
            // Las salidas las rellena `a_resultado`, que valida los `oneof`.
            salidas: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResultadoStep;

    #[test]
    fn ida_y_vuelta_con_medida() {
        let r = ResultadoStep::medido("medir_voltaje", "fail", "fuera", 4.2, 4.5, 5.5);
        let p: StepResult = (&r).into();
        assert_eq!(p.measured_value, "4.2");
        assert_eq!(p.limit_min, "4.5");
        assert_eq!(p.limit_max, "5.5");
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn ida_y_vuelta_sin_medida() {
        let r = ResultadoStep::nuevo("verificar_led", "pass", "led encendido");
        let p: StepResult = (&r).into();
        assert!(p.measured_value.is_empty());
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn campos_vacios_no_viajan() {
        // proto3: un string vacío no se serializa, así que un resultado
        // sin medida solo viaja con los tres primeros campos.
        let r = ResultadoStep::nuevo("x", "pass", "ok");
        let p: StepResult = (&r).into();
        let bytes = p.encode_to_vec();
        let redecodificado = StepResult::decode(&bytes[..]).unwrap();
        assert_eq!(redecodificado, p);
        // nombre + estado + mensaje y nada más.
        assert!(
            !bytes.windows(1).any(|w| w[0] == 0x22),
            "no debe haber tag 4"
        );
    }

    #[test]
    fn valor_ida_y_vuelta_por_los_tres_tipos() {
        for v in [
            expr::Value::Numero(4.2),
            expr::Value::Texto("banco-3".into()),
            expr::Value::Bool(true),
        ] {
            let cable = Value::desde_value("p", &v).expect("los tres tipos viajan");
            let bytes = cable.encode_to_vec();
            let vuelta = Value::decode(&bytes[..]).unwrap();
            assert_eq!(vuelta.name, "p");
            assert_eq!(vuelta.a_value(), Some(v));
        }
    }

    #[test]
    fn un_nulo_no_viaja() {
        // No tiene representación en el cable, y mandarlo como `oneof` vacío
        // sería mandar justo lo que el receptor tiene que rechazar.
        assert_eq!(Value::desde_value("p", &expr::Value::Nulo), None);
    }

    #[test]
    fn una_salida_sin_tipo_es_error_y_no_un_cero() {
        // Regla 2 de ADR-0019 en el cable de vuelta: un `oneof` sin rama no
        // dice de qué tipo es el valor. Tragárselo sería inventarse un dato
        // sobre la unidad bajo test.
        let p = StepResult {
            name: "medir".into(),
            status: "pass".into(),
            outputs: vec![Value {
                name: "tension".into(),
                dato: None,
            }],
            ..Default::default()
        };
        let e = p.a_resultado().expect_err("un oneof vacío no puede pasar");
        assert_eq!(e.nombre, "tension");
        assert!(
            e.to_string().contains("tension"),
            "el error nombra la salida"
        );
    }

    #[test]
    fn las_salidas_llegan_al_modelo_con_su_tipo() {
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
    fn un_ejecutor_de_contrato_1_devuelve_eco_cero() {
        // Lo que delata a un par antiguo: no conoce el tag 8, así que proto3
        // lo deja en el default. Es la base de la comprobación del eco que
        // hace el motor (ADR-0020 §4b).
        let viejo = StepResult {
            name: "verificar_led".into(),
            status: "pass".into(),
            message: "led encendido".into(),
            ..Default::default()
        };
        let bytes = viejo.encode_to_vec();
        let eco = StepResult::decode(&bytes[..]).unwrap().contract;
        assert_eq!(eco, 0, "el default de proto3 es lo que delata al par viejo");
        assert!(
            eco < CONTRATO,
            "y tiene que quedar por debajo del contrato de hoy, o el motor no \
             podría distinguirlo"
        );
    }

    #[test]
    fn los_campos_1_a_6_no_se_han_movido() {
        // El ADR dice que no se tocan, y desincronizar tags entre las cuatro
        // copias del contrato deja de ser un fallo de compilación para pasar
        // a ser un eco que miente.
        let r = ResultadoStep::medido("m", "pass", "ok", 4.2, 4.5, 5.5);
        let p: StepResult = (&r).into();
        let bytes = p.encode_to_vec();
        let vuelta = StepResult::decode(&bytes[..]).unwrap();
        assert_eq!(vuelta.measured_value, "4.2");
        assert_eq!(vuelta.limit_min, "4.5");
        assert_eq!(vuelta.limit_max, "5.5");
        assert_eq!(vuelta.contract, CONTRATO);
    }

    #[test]
    fn entero_sin_decimales() {
        assert_eq!(a_texto(Some(5.0)), "5");
        assert_eq!(a_texto(Some(4.2)), "4.2");
        assert_eq!(a_texto(None), "");
    }
}
