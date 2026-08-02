//! Helper de reintento de escritura ante fallos transitorios (RF-23,
//! MVP-parcial).
//!
//! El diseño (`docs/diseno/reportes.md` §"Reintento y reconexión") pide que
//! un sink que escribe a red/BD reintente los fallos transitorios sin
//! bloquear la ejecución. En el MVP no hay hilos (WASM/wasip2) ni sinks de
//! red: esto es la **infraestructura ligera** que usan los sinks de
//! fichero (JSON/CSV) y que quedará lista para los sinks de red futuros.
//!
//! Sin `sleep`: WASI Preview 2 no ofrece un mecanismo de espera fiable y
//! barato; el reintento es inmediato (best-effort). Los errores
//! **transitorios** (`Interrupted`, `WouldBlock`) se reintentan hasta
//! `intentos` veces; los **permanentes** (`NotFound`, `PermissionDenied`…)
//! fallan en el primer intento, sin gastar reintentos.

use std::io::{ErrorKind, Write};

/// `true` si el error merece otro intento (fue interrumpido o bloqueado,
/// no un fallo definitivo del destino).
fn es_transitorio(k: ErrorKind) -> bool {
    matches!(k, ErrorKind::Interrupted | ErrorKind::WouldBlock)
}

/// Escribe `datos` a `w`, reintentando hasta `intentos` veces los errores
/// transitorios. Los permanentes fallan en el primer intento. Sin espera
/// entre intentos (WASM). `intentos` es el número **total** de intentos
/// (1 = un solo tiro, sin reintento).
pub fn escribir_con_reintentos<W: Write>(
    w: &mut W,
    intentos: u32,
    datos: &[u8],
) -> std::io::Result<()> {
    let mut ultimo_error: Option<std::io::Error> = None;
    for _ in 0..intentos {
        match w.write_all(datos) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let transitorio = es_transitorio(e.kind());
                ultimo_error = Some(e);
                if !transitorio {
                    // Permanente: no gasta reintentos.
                    return Err(ultimo_error.unwrap());
                }
                // Transitorio: otro intento (si quedan).
            }
        }
    }
    Err(ultimo_error.expect("bucle con intentos ≥ 1 siempre entra al menos una vez"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Un writer que falla `fallos` veces con `WouldBlock` y luego escribe.
    struct EscribeTrasFallos {
        fallos_restantes: u32,
        escrito: Vec<u8>,
    }

    impl Write for EscribeTrasFallos {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.fallos_restantes > 0 {
                self.fallos_restantes -= 1;
                Err(ErrorKind::WouldBlock.into())
            } else {
                self.escrito.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn reintenta_hasta_lograrlo() {
        // 2 fallos transitorios; con 3 intentos llega a escribir.
        let mut w = EscribeTrasFallos { fallos_restantes: 2, escrito: Vec::new() };
        escribir_con_reintentos(&mut w, 3, b"hola").unwrap();
        assert_eq!(w.escrito, b"hola");
    }

    #[test]
    fn agota_intentos_y_falla() {
        // 2 fallos y solo 2 intentos: el último sigue fallando.
        let mut w = EscribeTrasFallos { fallos_restantes: 2, escrito: Vec::new() };
        let err = escribir_con_reintentos(&mut w, 2, b"hola").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WouldBlock);
        assert!(w.escrito.is_empty());
    }

    /// Un writer que siempre falla con un error permanente (`NotFound`).
    struct SiemprePermanente;
    impl Write for SiemprePermanente {
        fn write(&mut self, _b: &[u8]) -> std::io::Result<usize> {
            Err(ErrorKind::NotFound.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(ErrorKind::NotFound.into())
        }
    }

    #[test]
    fn error_permanente_no_reintenta() {
        let mut w = SiemprePermanente;
        // Aunque pidamos 5 intentos, un permanente falla en el primero.
        let err = escribir_con_reintentos(&mut w, 5, b"hola").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }
}