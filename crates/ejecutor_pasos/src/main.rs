//! Servidor gRPC que ejecuta pasos por nombre: el adaptador entre el motor
//! genérico y los pasos concretos, sobre `wasi-grpc`.
//!
//! Correr (desde la raíz del repo):
//!   cargo build --target wasm32-wasip2 -p ejecutor_pasos
//!   wasmtime -S cli -S tcp=y -S inherit-network=y \
//!     target/wasm32-wasip2/debug/ejecutor_pasos.wasm

use modelo::proto::{PeticionPaso, ResultadoPasoProto, RUTA_INVOCA};
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
/// y el paso sigue siendo opaco (ADR-0003/0005). `paso.proto` no cambia.
fn despacha(nombre: &str, intento: i32) -> modelo::ResultadoStep {
    if let Some(r) = pasos_scpi::despacha(nombre, intento) {
        return r;
    }
    pasos_demo::despacha(nombre, intento)
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

        loop {
            // Un error aquí es normalmente el motor cerrando la conexión al
            // acabar la secuencia: se sale del bucle sin ruido.
            let peticion = match conn.siguiente_peticion() {
                Ok(p) => p,
                Err(_) => break,
            };

            if peticion.path != RUTA_INVOCA {
                eprintln!("ruta desconocida: {}", peticion.path);
                continue;
            }

            let pet = match PeticionPaso::decode(&peticion.cuerpo[..]) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("petición ilegible: {e}");
                    continue;
                }
            };
            eprintln!("paso pedido: {} intento={}", pet.nombre, pet.intento);

            let resultado = despacha(&pet.nombre, pet.intento);
            let respuesta: ResultadoPasoProto = (&resultado).into();

            if let Err(e) = conn.responder(peticion.stream, &respuesta.encode_to_vec()) {
                eprintln!("error respondiendo: {e}");
                break;
            }
            eprintln!("respuesta enviada, stream {}", peticion.stream);
        }
        eprintln!("conexión cerrada; esperando otra");
    }
}
