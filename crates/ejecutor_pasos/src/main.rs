//! Servidor gRPC que ejecuta pasos por nombre: el adaptador entre el motor
//! genérico y los pasos concretos, sobre `wasi-grpc`.
//!
//! Correr (desde la raíz del repo):
//!   cargo build --target wasm32-wasip2 -p ejecutor_pasos
//!   wasmtime -S cli -S tcp=y -S inherit-network=y \
//!     target/wasm32-wasip2/debug/ejecutor_pasos.wasm

use expr::Value;
use modelo::proto::{StepRequest, StepResult, RUTA_INVOCA};
use modelo::ResultadoStep;
use prost::Message;
use wasi_grpc::grpc::Servidor;

/// Puerto por defecto (compat con `wasmtime run` sin host). El host (M5-ext.2,
/// ADR-0014) inyecta `ANVIL_PORT` para darle a cada `.wasm` un puerto efímero
/// propio; un `.wasm` de paso cargado por path es igual a éste.
const PUERTO_DEFECTO: u16 = 9100;

/// Despacho por nombre: raíz de composición del adapter (M5, RF-36).
/// Consulta primero el adapter **real** (`pasos_scpi`: instrumento por
/// SCPI/TCP) y luego los pasos **simulados** (`pasos_demo`). Un nombre
/// desconocido en ambos cae a `error` en `pasos_demo::despacha` (no
/// pánico: RF-12). El motor no cambia: sigue pidiendo `nombre` por gRPC
/// y el paso sigue siendo opaco (ADR-0003/0005).
///
/// Desde ADR-0020 el despacho lleva también los `parametros` de la petición,
/// ya evaluados por el motor. `pasos_scpi` todavía no los usa —su dirección
/// sigue viniendo de `ANVIL_SCPI_ADDR`, que es configuración de despliegue y
/// no un parámetro del paso— así que se le pasan sólo a `pasos_demo`.
fn despacha(nombre: &str, intento: i32, parametros: &[(String, Value)]) -> ResultadoStep {
    if let Some(r) = pasos_scpi::despacha(nombre, intento) {
        return r;
    }
    pasos_demo::despacha(nombre, intento, parametros)
}

/// Traduce los parámetros del cable a valores del motor (ADR-0020).
///
/// `Err(nombre)` si uno llegó con el `oneof` sin rama: no se puede saber de
/// qué tipo es, y un paso que mide sin un parámetro que le mandaron mide otra
/// cosa. Devuelve el nombre para poder nombrarlo en el error.
fn parametros_de(pet: &StepRequest) -> Result<Vec<(String, Value)>, String> {
    pet.inputs
        .iter()
        .map(|v| {
            v.a_value()
                .map(|valor| (v.name.clone(), valor))
                .ok_or_else(|| v.name.clone())
        })
        .collect()
}

fn main() {
    let puerto = std::env::var("ANVIL_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(PUERTO_DEFECTO);
    let servidor = match Servidor::escuchar("127.0.0.1", puerto) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo escuchar: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("ejecutor de pasos escuchando en {puerto}");

    // Acepta conexiones en bucle. El host embebido (ADR-0011) hace un
    // `connect` de prueba para esperar a que el ejecutor escuche antes de
    // lanzar el motor: con un único `aceptar` esa prueba consumiría la
    // conexión del motor. El bucle la descarta (se cierra sin peticiones) y
    // vuelve a aceptar la del motor. Para el smoke de dos terminales, el
    // ejecutor ya no termina solo: Ctrl-C cuando se acabe.
    //
    // Los logs van a **stderr**: stdout es territorio del motor (el reporte);
    // en el host embebido ambos guests comparten stdio.
    loop {
        let mut conn = match servidor.aceptar() {
            Ok(c) => c,
            Err(e) => {
                // La sonda del host embebido (connect de prueba que se cierra
                // sin preface HTTP/2) llega como "stream cerrado"/"EOF". No es
                // un error real: se silencia.
                let msg = format!("{e}");
                if !msg.contains("cerrado") && !msg.contains("closed") && !msg.contains("EOF") {
                    eprintln!("no se pudo aceptar: {e}");
                }
                continue;
            }
        };
        eprintln!("motor conectado");

        // Un error al leer es normalmente el motor cerrando la conexión al
        // acabar la secuencia: se sale del bucle sin ruido.
        while let Ok(peticion) = conn.siguiente_peticion() {
            if peticion.path != RUTA_INVOCA {
                eprintln!("ruta desconocida: {}", peticion.path);
                continue;
            }

            let pet = match StepRequest::decode(&peticion.cuerpo[..]) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("petición ilegible: {e}");
                    continue;
                }
            };
            eprintln!("paso pedido: {} intento={}", pet.name, pet.attempt);

            // ADR-0020: los parámetros llegan tipados y ya evaluados. Uno con
            // el `oneof` sin rama no dice de qué tipo es, y ejecutar el paso
            // sin él sería medir otra cosa en silencio: es `error` (Regla 2
            // de ADR-0019), no un valor por defecto.
            let resultado = match parametros_de(&pet) {
                Ok(parametros) => despacha(&pet.name, pet.attempt, &parametros),
                Err(nombre) => ResultadoStep::nuevo(
                    &pet.name,
                    "error",
                    format!(
                        "el parámetro '{nombre}' llegó sin tipo (ninguna de las ramas numero/\
                         texto/booleano): el paso no puede saber con qué medir"
                    ),
                ),
            };
            // El eco lo pone `From<&ResultadoStep>`: este ejecutor entiende el
            // contrato de `modelo::proto::CONTRATO`, y decirlo es lo que
            // permite al motor detectar a un par que no lo entiende.
            let respuesta: StepResult = (&resultado).into();

            if let Err(e) = conn.responder(peticion.stream, &respuesta.encode_to_vec()) {
                eprintln!("error respondiendo: {e}");
                break;
            }
            eprintln!("respuesta enviada, stream {}", peticion.stream);
        }
        eprintln!("conexión cerrada; esperando otra");
    }
}
