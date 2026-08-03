//! Host nativo de Anvil (ADR-0011): un único binario que **hospeda wasmtime
//! como librería** y orquesta los dos guests WASM — `anvil-guest.wasm` (motor)
//! y `ejecutor_pasos.wasm` (ejecutor) — embebidos en el propio binario.
//!
//! El usuario descarga un binario y corre:
//!
//! ```sh
//! ./anvil <secuencia.yaml> [--json <ruta>] [--csv <ruta>] [--limits <ruta>]
//! ```
//!
//! El host no parsea la línea de comandos (salvo `--solo-loopback`, que es
//! suyo): la pasa al guest motor (`inherit_args`), que la parsea como hoy.
//! El host sólo:
//!  1. Lee el YAML de la secuencia (M5-ext.1) para recolectar los
//!     `ejecutores:` — las IPs no-loopback declaradas (relajación acotada
//!     del loopback de ADR-0011: sólo se permiten las declaradas) y, en el
//!     futuro M5-ext.2, los `.wasm` por path a cargar.
//!  2. Arranca el ejecutor (thread) que bindea `127.0.0.1:9100` en su sandbox
//!     (loopback-only, sin relajación).
//!  3. Espera a que escuche (un `connect` de prueba; el ejecutor lo descarta).
//!  4. Arranca el motor (main) cuyo sandbox permite loopback **más** las IPs
//!     no-loopback declaradas en `ejecutores:` (sólo ésas). Conecta, corre la
//!     secuencia y sale.
//!  5. Propaga el exit del motor. El thread del ejecutor se aborta al acabar
//!     el proceso.
//!
//! Los guests hablan por gRPC sobre **loopback TCP** restringido
//! (`socket_addr_check → is_loopback`), salvo las IPs no-loopback
//! declaradas (ADR-0011, relajación acotada). El sandbox WASM y el
//! aislamiento motor↔ejecutor (dos `Store`s) se preservan.

use std::collections::HashSet;
use std::net::{IpAddr, TcpStream};
use std::thread;
use std::time::Duration;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// Guests embebidos (construidos a `wasm32-wasip2` y copiados a `OUT_DIR` por
/// `build.rs`).
const ANVIL_GUEST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anvil-guest.wasm"));
const EJECUTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ejecutor_pasos.wasm"));

const PUERTO: u16 = 9100;

/// Estado de cada guest: el contexto WASI (sockets/preopen/args) + la tabla
/// de recursos que `wasmtime-wasi` necesita.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

/// `WasiCtxBuilder` base con stdio heredado y **sockets restringidos a
/// loopback** (sólo `127.0.0.0/8` y `::1`). `inherit_network` da acceso a la
/// red del host; `socket_addr_check` rechaza cualquier IP no-loopback.
fn wasi_loopback() -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(|addr, _| Box::pin(async move { addr.ip().is_loopback() }));
    b
}

/// Como `wasi_loopback`, pero **relajando el loopback de forma acotada**
/// (ADR-0011, M5-ext.1): además de loopback, se permiten exactamente las
/// IPs no-loopback declaradas en `ejecutores:` del YAML. Sin declaración,
/// el comportamiento es idéntico a `wasi_loopback` (loopback-only).
fn wasi_loopback_con_declaradas(ips_declaradas: HashSet<IpAddr>) -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(move |addr, _| {
        let permitido = addr.ip().is_loopback() || ips_declaradas.contains(&addr.ip());
        Box::pin(async move { permitido })
    });
    b
}

/// IPs no-loopback declaradas en `ejecutores:` del YAML de la secuencia
/// (sólo `tipo: grpc` con `host` no-loopback). Es la "declaración" que
/// justifica la relajación del loopback de ADR-0011: nada sale de loopback
/// sin declararlo. Hosts que no parsean como IP (p. ej. `localhost`) no se
/// incluyen (el motor ya fallará al conectar; el sandbox no las deja pasar).
fn ips_no_loopback_declaradas(programa: &modelo::Programa) -> HashSet<IpAddr> {
    programa
        .ejecutores
        .values()
        .filter_map(|def| match &def.tipo {
            modelo::TipoEjecutor::Grpc { host, .. } => host
                .parse::<IpAddr>()
                .ok()
                .filter(|ip| !ip.is_loopback()),
            _ => None,
        })
        .collect()
}

/// Instancia y ejecuta un guest (componente WASI P2 `wasi:cli/run`) en su
/// propio `Store`. Devuelve el resultado de `call_run` para que el llamador
/// decida el exit. `bytes` = el `.wasm` embebido.
fn correr_guest(
    engine: &Engine,
    wasi: WasiCtx,
    bytes: &[u8],
) -> wasmtime::Result<Result<(), ()>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    let state = State { wasi, table: ResourceTable::new() };
    let mut store = Store::new(engine, state);
    let component = Component::from_binary(engine, bytes)?;
    let command = wasmtime_wasi::p2::bindings::sync::Command::instantiate(&mut store, &component, &linker)?;
    command.wasi_cli_run().call_run(&mut store)
}

/// Espera a que el ejecutor escuche en `127.0.0.1:PUERTO` con un `connect`
/// de prueba. El ejecutor (loop de aceptar) descarta esa conexión.
fn esperar_ejecutor() {
    let addr = format!("127.0.0.1:{PUERTO}");
    for _ in 0..500 {
        if let Ok(c) = TcpStream::connect(&addr) {
            drop(c); // conexión de prueba: se cierra; el ejecutor la descarta.
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    eprintln!("el ejecutor de pasos no empezó a escuchar en {addr}");
    std::process::exit(1);
}

fn main() {
    // El host parsea un único flag propio: `--solo-loopback` (rechaza
    // cualquier `grpc` no-loopback declarado, para CI/paranoia). El resto de
    // la línea de comandos se pasa tal cual al guest motor.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let solo_loopback = args.iter().any(|a| a == "--solo-loopback");
    let args_motor: Vec<String> =
        args.iter().filter(|a| *a != "--solo-loopback").cloned().collect();

    // M5-ext.1: leer el YAML para recolectar los `ejecutores:` declarados.
    // El primer argumento del motor es la ruta de la secuencia (si falta, el
    // motor ya se queja; aquí no hacemos nada especial).
    let mut ips_no_loopback: HashSet<IpAddr> = HashSet::new();
    if let Some(ruta) = args_motor.first() {
        match cargador::cargar_programa_de_archivo(ruta) {
            Ok(programa) => {
                ips_no_loopback = ips_no_loopback_declaradas(&programa);
                if solo_loopback && !ips_no_loopback.is_empty() {
                    let lista: Vec<String> = ips_no_loopback.iter().map(|i| i.to_string()).collect();
                    eprintln!(
                        "--solo-loopback: la secuencia declara ejecutores en IPs no-loopback ({})",
                        lista.join(", ")
                    );
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("aviso: no se pudo leer '{ruta}' para los ejecutores: {e}");
            }
        }
    }

    let engine = Engine::default();

    // --- Thread ejecutor: bind 127.0.0.1:9100 en su sandbox. ---
    // El ejecutor embebido sigue loopback-only (no atiende IPs externas).
    let exec_engine = engine.clone();
    let exec_handle = thread::spawn(move || {
        let wasi = wasi_loopback().build();
        // El ejecutor loop infinito de aceptar; su resultado se ignora.
        let _ = correr_guest(&exec_engine, wasi, EJECUTOR);
    });

    // --- Esperar a que escuche antes de lanzar el motor (que no reintenta). ---
    esperar_ejecutor();

    // --- Motor: hereda los args del host (secuencia + flags), preopen cwd.
    // --- Su sandbox permite loopback + las IPs no-loopback declaradas.
    let mut wasi = wasi_loopback_con_declaradas(ips_no_loopback);
    // argv[0] es el nombre del comando (el guest motor hace `args().skip(1)`).
    let mut argv: Vec<String> = vec!["anvil".to_string()];
    argv.extend(args_motor);
    wasi.args(&argv);
    if let Ok(cwd) = std::env::current_dir() {
        let _ = wasi.preopened_dir(&cwd, ".", DirPerms::all(), FilePerms::all());
    }
    let wasi = wasi.build();

    // El motor corre en el thread principal: su exit determina el del host.
    let r = correr_guest(&engine, wasi, ANVIL_GUEST);

    // El std de Rust en `wasm32-wasip2` normaliza `process::exit(non-zero)`
    // a `I32Exit(1)` (pérdida del código exacto, conocido en WASI P2). Así que
    // aquí propagamos 0 en éxito y el código del `I32Exit` (típicamente 1) en
    // fallo. El mensaje de error del motor va a stderr y guía al usuario.
    let exit_code = match r {
        Ok(Ok(())) => 0,
        Ok(Err(())) => 1,
        Err(e) => {
            if let Some(code) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                code.0
            } else {
                eprintln!("el motor falló: {e}");
                1
            }
        }
    };

    // El thread del ejecutor (loop infinito de aceptar) se aborta al salir.
    drop(exec_handle);
    std::process::exit(exit_code);
}