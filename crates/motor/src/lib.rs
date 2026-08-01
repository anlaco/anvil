//! Motor genérico de secuencias: lee una `DefinicionSecuencia` (datos) y la
//! corre invocando cada paso **por gRPC**, nunca con una llamada directa.
//!
//! Que todo paso pase por el cable es deliberado y es la decisión de
//! arquitectura del proyecto: aísla cada paso y deja la puerta abierta a
//! pasos escritos en cualquier lenguaje. El coste de una llamada local es
//! irrelevante frente al tiempo de un instrumento real.

use modelo::proto::{PeticionPaso, ResultadoPasoProto, RUTA_INVOCA};
use modelo::{DefinicionPaso, DefinicionSecuencia, ResultadoSecuencia, ResultadoStep};
use prost::Message;
use wasi_grpc::grpc::Cliente;
use wasi_grpc::net;

/// El motor: un cliente gRPC contra un ejecutor de pasos.
pub struct Motor {
    cliente: Cliente,
}

/// Qué salió mal al correr una secuencia. Un paso que *falla* no es un
/// error del motor — eso es un resultado válido; esto es que la
/// comunicación se rompió.
#[derive(Debug)]
pub enum Error {
    Red(net::Error),
    Protobuf(prost::DecodeError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Red(e) => write!(f, "{e}"),
            Error::Protobuf(e) => write!(f, "respuesta ilegible: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<net::Error> for Error {
    fn from(e: net::Error) -> Self {
        Error::Red(e)
    }
}

impl From<prost::DecodeError> for Error {
    fn from(e: prost::DecodeError) -> Self {
        Error::Protobuf(e)
    }
}

impl Motor {
    pub fn conecta(host: &str, puerto: u16) -> Result<Self, Error> {
        Ok(Motor { cliente: Cliente::conectar(host, puerto)? })
    }

    /// Invoca un paso por nombre. Cada llamada gasta un stream HTTP/2
    /// nuevo; de eso se encarga el cliente de `wasi-grpc`.
    fn ejecuta_paso(&mut self, nombre: &str, intento: i32) -> Result<ResultadoStep, Error> {
        let peticion = PeticionPaso { nombre: nombre.to_string(), intento };
        let bytes = self.cliente.unaria(RUTA_INVOCA, &peticion.encode_to_vec())?;
        Ok(ResultadoPasoProto::decode(&bytes[..])?.into())
    }

    /// Corre un paso hasta que pase o se agoten los intentos.
    /// `reintentos` es el número **total** de intentos: 1 = un solo tiro.
    fn ejecuta_con_reintentos(&mut self, def: &DefinicionPaso) -> Result<ResultadoStep, Error> {
        let max = def.reintentos.max(1);
        let mut resultado = self.ejecuta_paso(&def.nombre, 1)?;
        let mut intento = 1;
        while !resultado.paso() && intento < max {
            intento += 1;
            resultado = self.ejecuta_paso(&def.nombre, intento as i32)?;
        }
        Ok(resultado)
    }

    /// Corre una secuencia completa. La semántica es la de la spec y no
    /// cambia con la migración:
    ///
    /// - **Setup**: corren todos; si alguno no pasa, el Main se salta entero.
    /// - **Main**: solo si el Setup fue bien, y **corta en el primer fallo**.
    /// - **Cleanup**: corre siempre, pase lo que pase antes.
    pub fn ejecuta_secuencia(
        &mut self,
        definicion: &DefinicionSecuencia,
    ) -> Result<ResultadoSecuencia, Error> {
        let mut secuencia = ResultadoSecuencia::nueva(&definicion.nombre);

        let mut setup_ok = true;
        for p in &definicion.pasos_setup {
            let r = self.ejecuta_con_reintentos(p)?;
            if !r.paso() {
                setup_ok = false;
            }
            secuencia.registra(r);
        }

        if setup_ok {
            for p in &definicion.pasos_main {
                let r = self.ejecuta_con_reintentos(p)?;
                let corta = !r.paso();
                secuencia.registra(r);
                if corta {
                    break;
                }
            }
        }

        // Cleanup siempre: un equipo que se quedó encendido es peor que una
        // secuencia que falló.
        for p in &definicion.pasos_cleanup {
            let r = self.ejecuta_con_reintentos(p)?;
            secuencia.registra(r);
        }

        Ok(secuencia)
    }
}
