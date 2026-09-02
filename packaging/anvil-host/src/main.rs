//! Anvil's native host (ADR-0011): a single binary that **hosts wasmtime as
//! a library** and orchestrates the two WASM guests — `anvil-guest.wasm`
//! (engine) and `ejecutor_pasos.wasm` (executor) — embedded in the binary
//! itself.
//!
//! The user downloads a binary and runs:
//!
//! ```sh
//! ./anvil <sequence.yaml> [--json <path>] [--csv <path>] [--limits <path>]
//! ```
//!
//! The host does not parse the command line (except `--loopback-only`, which
//! is its own): it hands it to the engine guest (`inherit_args`), which
//! parses it as before. The host only:
//!  1. Reads the sequence's YAML (M5-ext.1/2) to collect the declared
//!     `executors:` — the declared non-loopback IPs (ADR-0011's bounded
//!     relaxation: only the declared ones are allowed) and, in M5-ext.2, the
//!     `.wasm` files to load by path.
//!  2. Starts the executor (thread) that binds `127.0.0.1:<port>` in its
//!     sandbox (loopback-only, no relaxation). The port is ephemeral per
//!     process unless the user pins it with `--port` (#15), and the host
//!     hands it to the engine so both ends agree. That decision comes
//!     **from the arguments alone**: if step 1 could not read the YAML, the
//!     executor starts all the same — the host does not predict the guest's
//!     verdict (#52).
//!  3. Waits for it to listen (a probe `connect`; the executor discards it).
//!  4. **M5-ext.2 (ADR-0015):** instantiates every `tipo: wasm` executor in
//!     the YAML by spawning the **bridge** `anvil-exec-wasm` with
//!     `--wasm <path> --port <ephemeral>`. The bridge loads the user's
//!     `.wasm` component (the `anvil:paso` interface, a `run` function) and
//!     serves it as gRPC on loopback. Waits for each one (readiness).
//!  5. Starts the engine (main) whose sandbox allows loopback **plus** the
//!     non-loopback IPs declared in `executors:` (only those). Connects,
//!     runs the sequence and exits.
//!  6. Propagates the engine's exit. The executor threads get aborted when
//!     the process ends.
//!
//! The guests speak over gRPC on **restricted loopback TCP**
//! (`socket_addr_check → is_loopback`), except for the non-loopback IPs
//! declared in `executors:` (ADR-0011, bounded relaxation). The WASM sandbox
//! and the engine↔executor isolation (one `Store` per guest) are preserved.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// Embedded guests (built for `wasm32-wasip2` and copied into `OUT_DIR` by
/// `build.rs`). The bridge binary is NOT embedded: it is a product that ships
/// as a file next to this one (ADR-0023) and gets looked up at spawn time.
const ANVIL_GUEST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anvil-guest.wasm"));
const EJECUTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ejecutor_pasos.wasm"));

/// Port of the embedded executor when the user pins it with `--port`.
/// Without that flag an **ephemeral** port per process is used (see
/// [`reserve_port`]): with a fixed port, two `anvil` processes could not
/// coexist — the second died with `address in use`, which is what blocked
/// parallelizing a campaign by launching N processes (#15). 9100 remains the
/// loose guest executor's default, for the two-terminal README flow.
const COMPAT_PORT: u16 = 9100;

/// Each guest's state: the WASI context (sockets/preopens/args) + the
/// resource table `wasmtime-wasi` needs.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// The `WasiCtxBuilder` base with inherited stdio and **sockets restricted to
/// loopback** (only `127.0.0.0/8` and `::1`). `inherit_network` grants access
/// to the host's network; `socket_addr_check` rejects any non-loopback IP.
fn wasi_loopback() -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(|addr, _| Box::pin(async move { addr.ip().is_loopback() }));
    b
}

/// Like `wasi_loopback`, but **relaxing loopback in a bounded way**
/// (ADR-0011, M5-ext.1): besides loopback, exactly the non-loopback IPs
/// declared in the sequence's `executors:` are allowed. With no declaration,
/// behavior is identical to `wasi_loopback` (loopback-only).
fn wasi_loopback_con_declaradas(ips_declaradas: HashSet<IpAddr>) -> WasiCtxBuilder {
    let mut b = WasiCtx::builder();
    b.inherit_stdio().inherit_network();
    b.socket_addr_check(move |addr, _| {
        let permitido = addr.ip().is_loopback() || ips_declaradas.contains(&addr.ip());
        Box::pin(async move { permitido })
    });
    b
}

/// CLI flags of the engine guest that **consume the next argument**
/// (M5, RF-40). The host only knows them to tell which argument is the
/// sequence's path; the real parsing is the guest's job.
const FLAGS_CON_VALOR: [&str; 6] = [
    "--process-model",
    "--json",
    "--csv",
    "--limits",
    "--executor",
    "--port",
];

/// The port the user pinned with `--port`, if any.
///
/// The flag used to only tell the **engine** where to connect, while the
/// host bound 9100 come what may: `anvil x.yaml --port 9200` brought the
/// executor up on 9100, the engine looked for 9200, and `connection refused`
/// came out. The guide even recommended it as a fix for port clashes, and it
/// did not work. It now pins **both ends**.
fn puerto_pedido(args: &[String]) -> Option<u16> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--port" {
            return it.next().and_then(|v| v.parse().ok());
        }
    }
    None
}

/// Reserves an ephemeral loopback port and returns it. The same mechanism the
/// host already used for the `.wasm` bridges (`instanciar_wasm`): binding
/// port 0 lets the OS pick a free one, it gets read back and released for the
/// guest to take. The window between the `drop` and the guest's `bind` is the
/// same as the bridge's, and it has given no trouble.
fn reservar_puerto() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("no se pudo reservar puerto para el ejecutor embebido: {e}"))?;
    let puerto = listener
        .local_addr()
        .map_err(|e| format!("no se pudo leer el puerto reservado: {e}"))?
        .port();
    drop(listener);
    Ok(puerto)
}

/// If the engine is going to exit without invoking a step, **nothing that
/// serves steps** should come up: neither the embedded executor nor the
/// bridges of the declared `.wasm` files. Starting them announces
/// `escuchando en …` ahead of the help or the verdict, and with the MVP's
/// fixed port it would block another `anvil` that did mean to run. Covers
/// what is decided **by the arguments alone** —`-h`, `-V`, `--validate`
/// (which loads without connecting) and a missing sequence—; an unknown flag
/// does not, because the host does not parse the command line (that is the
/// guest's job) and duplicating the full flag set here would be a worse
/// trade.
///
/// Issue #22: this existed from the start, but it only guarded the embedded
/// executor. The `.wasm` loop never consulted it, so `anvil s.yaml
/// --validate` with a declared `tipo: wasm` spawned `anvil-exec-wasm`,
/// which bound an ephemeral port and printed two lines — exactly what the
/// manual promises `--validate` does not do, and in the scenario (CI, no
/// hardware) where it matters most.
///
/// Whether the `.wasm` **exists** is still checked under `--validate`: that
/// is `EjecutorYaml::a_definicion`'s job in the loader, a file check that
/// needs neither instantiating wasmtime nor opening anything.
fn va_a_ejecutar_pasos(args: &[String]) -> bool {
    // ADR-0021: `--validate --with-executors` does not run a single step, but
    // it does **ask** the executors which steps they serve, and asking
    // requires connecting. The only exception, and an explicit one: whoever
    // typed it on the command line asked for it. Without the flag, `--validate`
    // still brings nothing up (issue #22).
    let pregunta_catalogos = args.iter().any(|a| a == "--with-executors");
    let mut it = args.iter();
    let mut hay_ruta = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--help" | "-h" | "--version" | "-V" => return false,
            "--validate" if !pregunta_catalogos => return false,
            // `--validate --with-executors` does not exit here: the path still
            // counts, like any run.
            "--validate" => {}
            f if FLAGS_CON_VALOR.contains(&f) => {
                it.next();
            }
            f if !f.starts_with('-') => hay_ruta = true,
            _ => {}
        }
    }
    hay_ruta
}

/// The sequence's path: the first **positional** argument, skipping flags and
/// their values. `None` if there is none, or if `--help`/`--version` was
/// requested (there is no YAML to pre-scan there, and warning that "no se
/// pudo leer '--help'" would only pollute the help).
fn ruta_de_secuencia(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--help" || a == "-h" || a == "--version" || a == "-V" {
            return None;
        }
        if FLAGS_CON_VALOR.contains(&a.as_str()) {
            it.next();
        } else if !a.starts_with('-') {
            return Some(a.clone());
        }
    }
    None
}

/// Non-loopback IPs declared in the sequence YAML's `executors:` (only
/// `tipo: grpc` with a non-loopback `host`). This is the "declaration" that
/// justifies ADR-0011's loopback relaxation: nothing leaves loopback without
/// being declared. Hosts that do not parse as an IP (e.g. `localhost`) are
/// not included (the engine will fail to connect anyway; the sandbox does not
/// let them through).
fn ips_no_loopback_declaradas(programa: &modelo::Programa) -> HashSet<IpAddr> {
    programa
        .ejecutores
        .values()
        .filter_map(|def| match &def.tipo {
            modelo::TipoEjecutor::Grpc { host, .. } => {
                host.parse::<IpAddr>().ok().filter(|ip| !ip.is_loopback())
            }
            _ => None,
        })
        .collect()
}

/// Instantiates and runs a guest (WASI P2 component, `wasi:cli/run`) in its
/// own `Store`. Returns the `call_run` result so the caller decides the exit
/// code. `bytes` = the embedded `.wasm`.
fn correr_guest(engine: &Engine, wasi: WasiCtx, bytes: &[u8]) -> wasmtime::Result<Result<(), ()>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    let state = State {
        wasi,
        table: ResourceTable::new(),
    };
    let mut store = Store::new(engine, state);
    let component = Component::from_binary(engine, bytes)?;
    let command =
        wasmtime_wasi::p2::bindings::sync::Command::instantiate(&mut store, &component, &linker)?;
    command.wasi_cli_run().call_run(&mut store)
}

/// A `.wasm` executor loaded by path (M5-ext.2, ADR-0015): the host spawns
/// the **bridge** (`anvil-exec-wasm`, a file next to this binary —
/// ADR-0023), which loads the user's `.wasm` component (the `anvil:paso`
/// interface, a `run` function) and serves it as a gRPC executor on
/// loopback. The user's `.wasm` is NOT a gRPC server: it is a pure function;
/// the bridge translates gRPC↔function.
///
/// `puerto` is the assigned (ephemeral) port, for readiness and to expose it
/// to the engine. `_child` is kept alive to preserve the stdin pipe: if the
/// host dies, the pipe closes → EOF → the bridge exits on its own (no
/// orphans).
struct EjecutorWasm {
    nombre: String,
    path: String,
    puerto: u16,
    _child: std::process::Child,
}

/// Where the bridge binary should be: **next to this binary** (ADR-0023).
/// One mechanism for development and distribution alike — the pair travels
/// together in the tarball, and `make release` leaves it together in the
/// target directory too.
///
/// The error names the path that was looked at and how to get the file
/// there. It never speculates about contract versions: a missing file is not
/// an old executor, and the engine's contract echo (ADR-0020 §4b) is the one
/// that names both numbers when that is the case.
fn ruta_puente() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("no se pudo localizar el binario anvil: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "el binario anvil no tiene directorio".to_string())?;
    let ruta = dir.join("anvil-exec-wasm");
    if !ruta.exists() {
        return Err(format!(
            "no se encontró el ejecutor WASM en '{}'. anvil lo busca junto a sí mismo \
             (ADR-0023): copia ahí 'anvil-exec-wasm' — make release lo deja al lado \
             del binario; también puedes copiarlo de executors/wasm/target/.",
            ruta.display()
        ));
    }
    Ok(ruta)
}

/// Spawns the bridge for a `.wasm` executor declared by path (M5-ext.2,
/// ADR-0015; ADR-0025 for the directory case):
///
/// 1. Reserves an **ephemeral** loopback port (`bind 127.0.0.1:0`).
/// 2. Looks up the bridge binary next to this one (ADR-0023).
/// 3. Spawns `anvil-exec-wasm --wasm <file> --port <port>` — or
///    `--modules <dir>` when the path is a directory — with stdin piped: the
///    bridge exits on its own if the host dies (EOF).
///
/// **The path decides which of the two the executor is** (ADR-0025 §D2): a
/// file serves one module and its steps keep their bare names, which is what
/// every sequence written before ADR-0025 says; a directory serves every
/// `*.wasm` in it and its steps are named `<module>/<step>`. Deriving it from
/// what the YAML points at is what keeps the two modes from blending.
///
/// The bridge is the one loading the components into its own Store (empty
/// WASI sandbox: no files, no network — a component is a pure function).
fn instanciar_wasm(nombre: &str, path: &Path) -> Result<EjecutorWasm, String> {
    let es_directorio = path.is_dir();
    if !es_directorio {
        let bytes = std::fs::read(path).map_err(|e| {
            format!(
                "el ejecutor '{nombre}' ({}) no se pudo leer: {e}",
                path.display()
            )
        })?;
        drop(bytes); // el puente es quien lee el fichero; aquí sólo validamos.
    }

    // Reservar un puerto efímero de loopback para el puente.
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("no se pudo reservar puerto para el ejecutor '{nombre}': {e}"))?;
    let puerto = listener
        .local_addr()
        .map_err(|e| {
            format!("no se pudo leer el puerto reservado para el ejecutor '{nombre}': {e}")
        })?
        .port();
    drop(listener);

    let puente = ruta_puente()?;
    let child = std::process::Command::new(&puente)
        .args([
            if es_directorio { "--modules" } else { "--wasm" },
            &path.display().to_string(),
            "--port",
            &puerto.to_string(),
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("no se pudo lanzar el puente para '{nombre}': {e}"))?;

    Ok(EjecutorWasm {
        nombre: nombre.into(),
        path: path.display().to_string(),
        puerto,
        _child: child,
    })
}

/// 10 ms polls while waiting for an executor to start listening (60 s).
/// Generous on purpose: in a **debug** build wasmtime compiles the guest
/// unoptimized and the executor can take tens of seconds to reach its `bind`
/// (in release it is immediate). The timeout only runs out when something is
/// really wrong, so overshooting costs nothing.
const SONDEOS_ARRANQUE: u32 = 6000;

/// Waits for the bridge of the `.wasm` executor to listen on its port (same
/// polling pattern as `wait_executor`). Timeout is per module; it fails with
/// a clear message naming the executor.
fn esperar_wasm(exec: &EjecutorWasm) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", exec.puerto);
    for _ in 0..SONDEOS_ARRANQUE {
        if let Ok(c) = TcpStream::connect(&addr) {
            drop(c); // conexión de prueba: se cierra; el puente la descarta.
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(format!(
        "el ejecutor '{}' ({}) no empezó a escuchar en {addr}",
        exec.nombre, exec.path
    ))
}

/// Waits for the executor to listen on `127.0.0.1:port` with a probe
/// `connect`. The executor (its accept loop) discards that connection.
fn esperar_ejecutor(puerto: u16) {
    let addr = format!("127.0.0.1:{puerto}");
    for _ in 0..SONDEOS_ARRANQUE {
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
    // The host parses a single flag of its own: `--loopback-only` (rejects
    // any declared non-loopback `grpc`, for CI/paranoia). The rest of the
    // command line goes through to the engine guest as-is.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let solo_loopback = args.iter().any(|a| a == "--loopback-only");
    let args_motor: Vec<String> = args
        .iter()
        .filter(|a| *a != "--loopback-only")
        .cloned()
        .collect();

    // M5-ext.1/2: read the YAML to collect the declared `executors:`. The
    // path is the first **positional** argument (M5, RF-40: the CLI accepts
    // flags before the sequence); if it is missing, or only help was asked
    // for, there is nothing to pre-scan and the engine guest takes over.
    let mut ips_no_loopback: HashSet<IpAddr> = HashSet::new();
    let mut programa: Option<modelo::Programa> = None;
    let ruta_secuencia = ruta_de_secuencia(&args_motor);
    if let Some(ruta) = ruta_secuencia.as_ref() {
        match cargador::cargar_programa_de_archivo(ruta) {
            Ok(p) => {
                ips_no_loopback = ips_no_loopback_declaradas(&p);
                if solo_loopback && !ips_no_loopback.is_empty() {
                    let lista: Vec<String> =
                        ips_no_loopback.iter().map(|i| i.to_string()).collect();
                    eprintln!(
                        "--loopback-only: la secuencia declara ejecutores en IPs no-loopback ({})",
                        lista.join(", ")
                    );
                    std::process::exit(1);
                }
                programa = Some(p);
            }
            // The engine guest re-parses the same YAML an instant later and
            // reports the error with its own wording. Repeating it here, and
            // on top as "no se pudo leer ... para los ejecutores" — when the
            // file was read perfectly fine and the failure is a schema one —
            // is the pattern DIAG-5 hunts down. Only warn about what the
            // guest would not see either: a read failure, where host and
            // guest differ because the guest looks inside its sandbox.
            Err(e @ cargador::ErrorCarga::Lectura(_)) => {
                eprintln!("aviso: no se pudo leer '{ruta}' para los ejecutores: {e}");
            }
            // Syntax/validation: the guest will reparse the same file and
            // report it with its own wording, so nothing is said here. What
            // is **not** done is deducing from this that the guest will fail
            // to load it too (#52): that deduction only holds while host and
            // guest share a loader, and when they do not the price is an
            // unexplained `connection-refused`. See `start_executor`.
            Err(_) => {}
        }
    }

    let engine = Engine::default();

    // --- M5-ext.2: instantiate the `tipo: wasm` executors declared in the
    // --- YAML and expose them to the engine as synthetic `--executor`
    // --- overrides. The engine guest re-parses the YAML itself (ADR-0005:
    // --- the engine is not handed an in-memory `Programa`), so the host
    // --- cannot rewrite its model: it composes
    // --- `--executor name=127.0.0.1:port` (M5-ext.1, which already turns
    // --- `wasm` into `grpc` when applying it).
    let ruta_yaml = ruta_secuencia.clone().unwrap_or_default();
    let dir_yaml = Path::new(&ruta_yaml)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    let mut ejecutores_wasm: Vec<EjecutorWasm> = Vec::new();
    let mut overrides_motor: Vec<String> = Vec::new();
    let mut args_motor_final: Vec<String> = args_motor;
    // Computed over `args_motor` (still without the synthetic `--executor`
    // flags, which is exactly what this block produces). It makes no
    // difference whether it is computed over `args_motor_final`: the guard
    // skips `--executor` and its value via `FLAGS_CON_VALOR`.
    //
    // Decided **from the arguments alone** (#52). It used to also require the
    // host to have parsed the YAML, which assumed a YAML the host rejects
    // would be rejected by the guest all the same. As soon as host and guest
    // stop sharing a loader — a half-built tree suffices — the premise is
    // false: the guest loads the sequence, nobody started the embedded
    // executor, and the user sees `connection-refused` against 9100 without
    // a single line saying why. The host does not predict the guest's
    // verdict.
    let va_a_ejecutar = va_a_ejecutar_pasos(&args_motor_final);
    if va_a_ejecutar {
        if let Some(p) = programa.as_ref() {
            // Deduplicate by path (two executors with the same `.wasm` → one
            // Store).
            let mut stores_por_path: HashMap<String, u16> = HashMap::new();
            let mut errores: Vec<String> = Vec::new();
            for (nombre, def) in &p.ejecutores {
                if let modelo::TipoEjecutor::Wasm { path } = &def.tipo {
                    // The loader already validated the path exists (fail-fast
                    // at load); here we resolve it relative to the YAML's
                    // directory.
                    let ruta = cargador::normalizar_path(&dir_yaml, Path::new(path));
                    let clave = ruta.to_string_lossy().into_owned();
                    let puerto = if let Some(puerto) = stores_por_path.get(&clave) {
                        *puerto
                    } else {
                        let exec = match instanciar_wasm(nombre, &ruta) {
                            Ok(e) => e,
                            Err(e) => {
                                errores.push(e);
                                continue;
                            }
                        };
                        let puerto = exec.puerto;
                        if let Err(e) = esperar_wasm(&exec) {
                            errores.push(e);
                            continue;
                        }
                        eprintln!(
                            "ejecutor '{}' cargado ({} → 127.0.0.1:{})",
                            nombre,
                            ruta.display(),
                            puerto
                        );
                        ejecutores_wasm.push(exec);
                        stores_por_path.insert(clave, puerto);
                        puerto
                    };
                    overrides_motor.push(format!("{nombre}=127.0.0.1:{puerto}"));
                }
            }
            if !errores.is_empty() {
                for e in &errores {
                    eprintln!("{e}");
                }
                std::process::exit(1);
            }
        }
    }
    for o in &overrides_motor {
        args_motor_final.push("--executor".into());
        args_motor_final.push(o.clone());
    }

    // --- Executor thread: binds inside its sandbox, loopback-only (it does
    // --- not serve external IPs). It does not start if the engine will never
    // --- invoke a step (help, version, `--validate`, missing sequence).
    //
    // The port is **ephemeral per process** unless the user pins it with
    // `--port`: that way two simultaneous `anvil` processes do not clash
    // (#15). It is handed to the executor via `ANVIL_PORT` — the channel
    // already used for path-loaded `.wasm` executors (ADR-0014) — and to the
    // engine as `--port`, which is how it locates the embedded executor.
    let arranca_ejecutor = va_a_ejecutar;
    let puerto_ejecutor = match puerto_pedido(&args_motor_final) {
        Some(p) => p,
        // Without an embedded executor there is nobody to assign a port to
        // (and reserving one would touch the network for nothing: DIAG-5f).
        None if !arranca_ejecutor => COMPAT_PORT,
        None => match reservar_puerto() {
            Ok(p) => {
                args_motor_final.push("--port".into());
                args_motor_final.push(p.to_string());
                p
            }
            // No ephemeral port available: the usual 9100. Worse is not
            // starting at all.
            Err(e) => {
                eprintln!("aviso: {e}; se usa el puerto {COMPAT_PORT}");
                COMPAT_PORT
            }
        },
    };
    let exec_handle = if arranca_ejecutor {
        let exec_engine = engine.clone();
        let h = thread::spawn(move || {
            let mut b = wasi_loopback();
            b.env("ANVIL_PORT", puerto_ejecutor.to_string());
            let wasi = b.build();
            // The executor is an infinite accept loop: if it ends, that is a
            // failure. The error goes to stderr — otherwise the user only
            // sees `wait_executor()`'s timeout without the cause.
            if let Err(e) = correr_guest(&exec_engine, wasi, EJECUTOR) {
                eprintln!("el ejecutor de pasos terminó con error: {e:?}");
            }
        });
        // --- Wait for it to listen before launching the engine (no retry).
        esperar_ejecutor(puerto_ejecutor);
        Some(h)
    } else {
        None
    };

    // --- Engine: inherits the host's args (sequence + flags), preopens cwd.
    // --- Its sandbox allows loopback + the declared non-loopback IPs.
    let mut wasi = wasi_loopback_con_declaradas(ips_no_loopback);
    // argv[0] is the command name (the engine guest does `args().skip(1)`).
    let mut argv: Vec<String> = vec!["anvil".to_string()];
    argv.extend(args_motor_final);
    wasi.args(&argv);
    if let Ok(cwd) = std::env::current_dir() {
        let _ = wasi.preopened_dir(&cwd, ".", DirPerms::all(), FilePerms::all());
    }
    let wasi = wasi.build();

    // The engine runs on the main thread: its exit determines the host's.
    let r = correr_guest(&engine, wasi, ANVIL_GUEST);

    // Rust's std on `wasm32-wasip2` normalizes `process::exit(non-zero)` to
    // `I32Exit(1)` (exact code lost, known in WASI P2). So we propagate 0 on
    // success and the `I32Exit` code (typically 1) on failure. The engine's
    // error message goes to stderr and guides the user.
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

    // The executor threads (infinite accept loops) get aborted on exit. The
    // loaded `.wasm` components stay in the process until then (preload,
    // TestStand's default); there is no orderly shutdown in the MVP.
    drop(exec_handle);
    drop(ejecutores_wasm);
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::ruta_de_secuencia;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ruta_sola() {
        assert_eq!(ruta_de_secuencia(&args(&["s.yaml"])), Some("s.yaml".into()));
    }

    #[test]
    fn flags_antes_de_la_ruta() {
        let a = args(&["--process-model", "pm.yaml", "--quiet", "s.yaml"]);
        assert_eq!(ruta_de_secuencia(&a), Some("s.yaml".into()));
    }

    #[test]
    fn el_valor_de_un_flag_no_es_la_ruta() {
        // `pm.yaml` is the value of `--process-model`, not the sequence.
        let a = args(&["--process-model", "pm.yaml"]);
        assert_eq!(ruta_de_secuencia(&a), None);
    }

    #[test]
    fn flags_despues_de_la_ruta() {
        let a = args(&["s.yaml", "--json", "o.json", "--csv", "o.csv"]);
        assert_eq!(ruta_de_secuencia(&a), Some("s.yaml".into()));
    }

    #[test]
    fn flag_desconocido_no_se_confunde_con_la_ruta() {
        // The guest will complain; the host only has to not take it for a
        // path.
        let a = args(&["--inventado", "s.yaml"]);
        assert_eq!(ruta_de_secuencia(&a), Some("s.yaml".into()));
    }

    #[test]
    fn sin_argumentos() {
        assert_eq!(ruta_de_secuencia(&[]), None);
    }

    #[test]
    fn los_flags_cortos_no_son_la_ruta() {
        // DIAG-5: `-h` is not the path of a sequence called `-h`, nor `-x`
        // that of a file called `-x`; both are the guest parser's business.
        assert_eq!(ruta_de_secuencia(&args(&["-h"])), None);
        assert_eq!(ruta_de_secuencia(&args(&["-V"])), None);
        assert_eq!(ruta_de_secuencia(&args(&["-x"])), None);
        assert_eq!(
            ruta_de_secuencia(&args(&["-x", "s.yaml"])),
            Some("s.yaml".into())
        );
    }

    #[test]
    fn help_y_version_no_tienen_ruta() {
        assert_eq!(ruta_de_secuencia(&args(&["--help"])), None);
        assert_eq!(ruta_de_secuencia(&args(&["--version"])), None);
        // Also when they trail the sequence: the guest exits via help and the
        // host must not pre-scan or complain about the YAML.
        assert_eq!(
            ruta_de_secuencia(&args(&["s.yaml", "--help"])),
            Some("s.yaml".into())
        );
    }

    /// The embedded executor opens a fixed port and announces it on stderr:
    /// it must not start when the engine will exit without invoking any step.
    #[test]
    fn el_ejecutor_embebido_solo_arranca_si_hay_pasos_que_correr() {
        use super::va_a_ejecutar_pasos as necesita;
        assert!(necesita(&args(&["s.yaml"])));
        assert!(necesita(&args(&["--process-model", "pm.yaml", "s.yaml"])));
        // Help and version: the engine prints and exits.
        assert!(!necesita(&args(&["-h"])));
        assert!(!necesita(&args(&["--help"])));
        assert!(!necesita(&args(&["-V"])));
        assert!(!necesita(&args(&["s.yaml", "--version"])));
        // `--validate` loads and validates without connecting to anyone.
        assert!(!necesita(&args(&["s.yaml", "--validate"])));
        // No sequence, nothing to run.
        assert!(!necesita(&args(&[])));
        assert!(!necesita(&args(&["--quiet"])));
        // And a flag's value does not count as a sequence.
        assert!(!necesita(&args(&["--json", "o.json"])));
    }
}
