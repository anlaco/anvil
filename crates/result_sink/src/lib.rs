//! Los ResultSinks de anvil: consumidores del resultado de la secuencia
//! que implementan el trait `modelo::ResultSink`. El motor publica
//! eventos; estos sinks los vierten a consola, JSON o CSV.
//!
//! SQLite queda **aplazado** (ADR-0007): no compila en `wasm32-wasip2` en
//! este toolchain (SQLite es C; `rusqlite` bundled necesita `cc` para el
//! target wasm). El trait ya está fijado para añadirlo después como un
//! sink más.
//!
//! Todos los sinks son genéricos sobre `W: std::io::Write` para ser
//! testeables con un `Vec<u8>` sin tocar el disco. Los sinks de fichero
//! (JSON/CSV) reintentan la escritura ante fallos transitorios (RF-23,
//! ver `reintento`).

pub mod consola;

pub use consola::SinkConsola;