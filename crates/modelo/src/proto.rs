//! Los mensajes de `paso.proto`, declarados a mano con `prost` (v0.1 de
//! wasi-grpc no trae codegen). El `.proto` es la fuente de verdad del
//! contrato: si se toca uno, hay que tocar el otro.
//!
//! Los tres campos de medida van como `string` porque así los definió el
//! contrato. En proto3 un `string` vacío no se transmite.

use prost::Message;

/// La ruta del método gRPC. Sin `package` en el `.proto`, así que es
/// directamente `/<servicio>/<método>`.
pub const RUTA_INVOCA: &str = "/EjecutorPasos/Invoca";

#[derive(Clone, PartialEq, Message)]
pub struct PeticionPaso {
    #[prost(string, tag = "1")]
    pub nombre: String,
    /// Número de intento, empezando en 1. Los pasos lo reciben para poder
    /// simular fallos transitorios (ver `pasos_demo`).
    #[prost(int32, tag = "2")]
    pub intento: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResultadoPasoProto {
    #[prost(string, tag = "1")]
    pub nombre: String,
    #[prost(string, tag = "2")]
    pub estado: String,
    #[prost(string, tag = "3")]
    pub mensaje: String,
    #[prost(string, tag = "4")]
    pub valor_medido: String,
    #[prost(string, tag = "5")]
    pub limite_min: String,
    #[prost(string, tag = "6")]
    pub limite_max: String,
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

impl From<&crate::ResultadoStep> for ResultadoPasoProto {
    fn from(r: &crate::ResultadoStep) -> Self {
        ResultadoPasoProto {
            nombre: r.nombre.clone(),
            estado: r.estado.clone(),
            mensaje: r.mensaje.clone(),
            valor_medido: a_texto(r.valor_medido),
            limite_min: a_texto(r.limite_min),
            limite_max: a_texto(r.limite_max),
        }
    }
}

impl From<ResultadoPasoProto> for crate::ResultadoStep {
    fn from(p: ResultadoPasoProto) -> Self {
        crate::ResultadoStep {
            nombre: p.nombre,
            estado: p.estado,
            mensaje: p.mensaje,
            valor_medido: de_texto(&p.valor_medido),
            limite_min: de_texto(&p.limite_min),
            limite_max: de_texto(&p.limite_max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResultadoStep;

    #[test]
    fn ida_y_vuelta_con_medida() {
        let r = ResultadoStep::medido("medir_voltaje", "fallo", "fuera", 4.2, 4.5, 5.5);
        let p: ResultadoPasoProto = (&r).into();
        assert_eq!(p.valor_medido, "4.2");
        assert_eq!(p.limite_min, "4.5");
        assert_eq!(p.limite_max, "5.5");
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn ida_y_vuelta_sin_medida() {
        let r = ResultadoStep::nuevo("verificar_led", "paso", "led encendido");
        let p: ResultadoPasoProto = (&r).into();
        assert!(p.valor_medido.is_empty());
        assert_eq!(ResultadoStep::from(p), r);
    }

    #[test]
    fn campos_vacios_no_viajan() {
        // proto3: un string vacío no se serializa, así que un resultado
        // sin medida solo viaja con los tres primeros campos.
        let r = ResultadoStep::nuevo("x", "paso", "ok");
        let p: ResultadoPasoProto = (&r).into();
        let bytes = p.encode_to_vec();
        let redecodificado = ResultadoPasoProto::decode(&bytes[..]).unwrap();
        assert_eq!(redecodificado, p);
        // nombre + estado + mensaje y nada más.
        assert!(!bytes.windows(1).any(|w| w[0] == 0x22), "no debe haber tag 4");
    }

    #[test]
    fn entero_sin_decimales() {
        assert_eq!(a_texto(Some(5.0)), "5");
        assert_eq!(a_texto(Some(4.2)), "4.2");
        assert_eq!(a_texto(None), "");
    }
}
