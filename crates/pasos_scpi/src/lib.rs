//! Pasos de instrumento **reales** por SCPI sobre TCP (M5, RF-36). Adapter
//! gRPC pulido: el motor invoca el paso por nombre como cualquier `grpc`
//! (ADR-0003/ADR-0005); el paso abre un `TcpStream`, envía un comando SCPI
//! y parsea la respuesta numérica. `paso.proto` **no cambia** (RNF-05): el
//! paso es opaco al motor, que sigue sin saber qué es SCPI ni TCP.
//!
//! Compila a `wasm32-wasip2` (`std::net` vía `wasi:sockets`; el host embebido
//! restringe los sockets a loopback, `packaging/anvil-host/src/main.rs`) y a
//! nativo (tests). Sin dependencias externas (ADR-0001). `record/replay` y
//! `wasi-visa` son post-MVP; aquí va la capa 1 de
//! `diseno/integracion-instrumentos.md`: un paso que habla de verdad con un
//! instrumento por TCP, en vez de un valor simulado en código.

use modelo::ResultadoStep;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Dirección del instrumento SCPI. En producción se sobreescribe con la
/// variable de entorno `ANVIL_SCPI_ADDR` (p. ej. `127.0.0.1:5025`); en el
/// sandbox del host embebido sólo se permite loopback (ADR-0011). El host
/// aún no plubea env vars al guest (post-MVP), así que en el binario único
/// rige `ADDR_DEFAULT`; el override es útil en el path de dos terminales
/// con wasmtime.
const ADDR_DEFAULT: &str = "127.0.0.1:5025";

fn addr() -> String {
    std::env::var("ANVIL_SCPI_ADDR").unwrap_or_else(|_| ADDR_DEFAULT.into())
}

/// Medición de voltaje real por SCPI/TCP contra `addr`. Conecta, envía
/// `MEASURE:VOLTAGE?\n`, lee la respuesta y la parsea como `f64`. Devuelve
/// `ResultadoStep::medido_valor` con `estado="paso"` si la medición fue
/// bien; `estado="error"` si la conexión, la E/S o el parseo fallan. El
/// `intento` se ignora: una medición puntual SCPI es idempotente, y los
/// reintentos los orquesta el motor (no el paso).
///
/// Versión parametrizable por dirección: los tests la usan con la
/// dirección del mock (puerto efímero), sin tocar la env var global — así
/// no hay carrera entre tests paralelos.
pub fn medir_voltaje_scpi_en(addr: &str, _intento: i32) -> ResultadoStep {
    let stream = match TcpStream::connect(addr) {
        Ok(s) => s,
        Err(e) => {
            return ResultadoStep::nuevo(
                "medir_voltaje_scpi",
                "error",
                format!("SCPI connect {addr} falló: {e}"),
            )
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut stream = stream;
    if let Err(e) = stream.write_all(b"MEASURE:VOLTAGE?\n") {
        return ResultadoStep::nuevo(
            "medir_voltaje_scpi",
            "error",
            format!("SCPI write falló: {e}"),
        );
    }
    let mut buf = [0u8; 64];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        Ok(_) => {
            return ResultadoStep::nuevo(
                "medir_voltaje_scpi",
                "error",
                "SCPI sin respuesta",
            )
        }
        Err(e) => {
            return ResultadoStep::nuevo(
                "medir_voltaje_scpi",
                "error",
                format!("SCPI read falló: {e}"),
            )
        }
    };
    let texto = std::str::from_utf8(&buf[..n]).unwrap_or("").trim();
    match texto.parse::<f64>() {
        Ok(v) => ResultadoStep::medido_valor(
            "medir_voltaje_scpi",
            "paso",
            format!("SCPI medido: {v} V"),
            v,
        ),
        Err(_) => ResultadoStep::nuevo(
            "medir_voltaje_scpi",
            "error",
            format!("SCPI respuesta no numérica: {texto:?}"),
        ),
    }
}

/// Medición de voltaje SCPI usando la dirección de `ANVIL_SCPI_ADDR` (o
/// `ADDR_DEFAULT`). Es la que despacha el ejecutor en producción.
pub fn medir_voltaje_scpi(intento: i32) -> ResultadoStep {
    medir_voltaje_scpi_en(&addr(), intento)
}

/// Despacho por nombre para el ejecutor. `None` = no es un paso SCPI (que
/// el llamador pruebe otros despachadores, p. ej. `pasos_demo`). Así el
/// ejecutor compone adaptadores sin que el motor se entere.
pub fn despacha(nombre: &str, intento: i32) -> Option<ResultadoStep> {
    match nombre {
        "medir_voltaje_scpi" => Some(medir_voltaje_scpi(intento)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Levanta un mock SCPI en loopback de puerto efímero y responde
    /// `responder` a un comando entrante. Determinista: ningún puerto fijo.
    /// Devuelve la dirección para pasársela a `medir_voltaje_scpi_en` —
    /// **sin tocar `ANVIL_SCPI_ADDR`**, así los tests no compiten por la
    /// env var al correr en paralelo.
    fn mock_scpi(responder: &'static str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            // accept con timeout corto: si el paso no conecta (p. ej. un
            // test fallido), el mock no cuelga el runner para siempre.
            let _ = listener.set_nonblocking(true);
            for _ in 0..200 {
                if let Ok((mut s, _)) = listener.accept() {
                    let mut buf = [0u8; 64];
                    let _ = s.read(&mut buf);
                    let _ = s.write_all(responder.as_bytes());
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        (addr, handle)
    }

    #[test]
    fn medir_voltaje_scpi_parsea_respuesta_numerica() {
        let (addr, h) = mock_scpi("4.8\n");
        let r = medir_voltaje_scpi_en(&addr, 1);
        h.join().unwrap();
        assert_eq!(r.estado, "paso");
        assert_eq!(r.valor_medido, Some(4.8));
        assert_eq!(r.nombre, "medir_voltaje_scpi");
    }

    #[test]
    fn medir_voltaje_scpi_respuesta_no_numerica_es_error() {
        let (addr, h) = mock_scpi("OVEN_COLD\n");
        let r = medir_voltaje_scpi_en(&addr, 1);
        h.join().unwrap();
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("no numérica"));
    }

    #[test]
    fn medir_voltaje_scpi_sin_servidor_es_error() {
        // Puerto 1: loopback, sin listener → connect falla.
        let r = medir_voltaje_scpi_en("127.0.0.1:1", 1);
        assert_eq!(r.estado, "error");
        assert!(r.mensaje.contains("connect"));
    }

    #[test]
    fn despacha_nombre_conocido_y_desconocido() {
        assert!(despacha("medir_voltaje_scpi", 1).is_some());
        assert!(despacha("no_es_scpi", 1).is_none());
        // Un paso demo conocido tampoco lo ataja pasos_scpi:
        assert!(despacha("medir_voltaje", 1).is_none());
    }
}