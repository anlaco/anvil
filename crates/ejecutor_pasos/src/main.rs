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

const PUERTO: u16 = 9100;

fn main() {
    let servidor = match Servidor::escuchar("127.0.0.1", PUERTO) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo escuchar: {e}");
            std::process::exit(1);
        }
    };
    println!("ejecutor de pasos escuchando en {PUERTO}");

    let mut conn = match servidor.aceptar() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("no se pudo aceptar: {e}");
            std::process::exit(1);
        }
    };
    println!("motor conectado");

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
        println!("paso pedido: {} intento={}", pet.nombre, pet.intento);

        let resultado = pasos_demo::despacha(&pet.nombre, pet.intento);
        let respuesta: ResultadoPasoProto = (&resultado).into();

        if let Err(e) = conn.responder(peticion.stream, &respuesta.encode_to_vec()) {
            eprintln!("error respondiendo: {e}");
            break;
        }
        println!("respuesta enviada, stream {}", peticion.stream);
    }

    println!("ejecutor de pasos cerrado");
}
