//! Anvil's native host (ADR-0011): a single binary that **hosts wasmtime as
//! a library** and runs the engine guest, `anvil-guest.wasm`, embedded in the
//! binary itself. It carries no step executor (ADR-0041).
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
//!  2. **M5-ext.2 (ADR-0015, ADR-0027):** instantiates every `tipo: wasm`
//!     executor in the YAML by spawning **the binary the sequence names in its
//!     `path:`** with `--port <ephemeral>`, and nothing else: which modules
//!     that executor serves is its own business — it finds them next to its
//!     binary. Waits for each one (readiness).
//!  3. Starts the engine (main) whose sandbox allows loopback **plus** the
//!     non-loopback IPs declared in `executors:` (only those). Connects,
//!     runs the sequence and exits.
//!  4. Propagates the engine's exit. The spawned executors exit when the host
//!     does (their stdin closes).
//!
//! The engine speaks gRPC to the executors on **restricted loopback TCP**
//! (`socket_addr_check → is_loopback`), except for the non-loopback IPs
//! declared in `executors:` (ADR-0011, bounded relaxation).

use std::collections::HashSet;
use std::net::{IpAddr, TcpListener};
use std::path::{Path, PathBuf};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bridge;

/// The embedded engine guest (built for `wasm32-wasip2` and copied into
/// `OUT_DIR` by `build.rs`). The bridge binary is NOT embedded: it ships as a file next to
/// this one (ADR-0023), and from there it gets **copied into whatever folder
/// is to be a department** — the sequence names the binary to spawn
/// (ADR-0027), so there is no lookup here any more.
const ANVIL_GUEST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anvil-guest.wasm"));

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

/// The engine's `WasiCtxBuilder`: inherited stdio and sockets restricted to
/// loopback, **relaxed in a bounded way** (ADR-0011, M5-ext.1): besides
/// loopback, exactly the non-loopback IPs declared in the sequence's
/// `executors:` are allowed. With no declaration, loopback only.
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
const FLAGS_CON_VALOR: [&str; 5] = [
    "--process-model",
    "--json",
    "--csv",
    "--limits",
    "--executor",
];

/// The sequence's path: the first **positional** argument, skipping flags and
/// their values. `None` if there is none, or if `--help`/`--version` was
/// requested (there is no YAML to pre-scan there, and warning that "no se
/// pudo leer '--help'" would only pollute the help).
/// The words this CLI accepts in first position that are not a path and not a
/// flag. Exactly one today (ADR-0044).
const SUBCOMANDOS: [&str; 1] = ["describe"];

/// The arguments with a leading subcommand dropped.
///
/// Everything in this file that reads the arguments is looking for the
/// sequence and its flags, and a subcommand is neither. Stripping it once,
/// here, is what keeps `describe` inheriting `--process-model`, `--limits`,
/// `--executor` and the preopen logic without any of them learning the word.
fn sin_subcomando(args: &[String]) -> &[String] {
    match args.first() {
        Some(a) if SUBCOMANDOS.contains(&a.as_str()) => &args[1..],
        _ => args,
    }
}

fn ruta_de_secuencia(args: &[String]) -> Option<String> {
    let args = sin_subcomando(args);
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

/// The subset of `FLAGS_CON_VALOR` whose value is a filesystem path, as
/// opposed to `--executor` (a `name=addr` pair with no path in it).
const FLAGS_DE_RUTA: [&str; 4] = ["--process-model", "--json", "--csv", "--limits"];

/// Every path-valued argument bound for the engine guest: the sequence's own
/// path plus the value of each `FLAGS_DE_RUTA` flag present. Used to decide
/// which extra directories the host must preopen (issue #40): only the cwd
/// is preopened under the guest name `"."`, so an **absolute** path does not
/// match that preopen's prefix and WASI rejects it (`os error 44`) even when
/// the file exists — see the preopen loop in `main`.
fn rutas_de_argumentos(args: &[String]) -> Vec<String> {
    let mut rutas: Vec<String> = ruta_de_secuencia(args).into_iter().collect();
    let args = sin_subcomando(args);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if FLAGS_DE_RUTA.contains(&a.as_str()) {
            if let Some(v) = it.next() {
                rutas.push(v.clone());
            }
        } else if FLAGS_CON_VALOR.contains(&a.as_str()) {
            it.next();
        }
    }
    rutas
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
        .filter_map(|def| {
            // One kind since ADR-0046, so there is nothing left to filter out
            // here — only hosts that do not parse as an IP (`localhost`) and
            // the loopback ones.
            let modelo::TipoEjecutor::Grpc { host, .. } = &def.tipo;
            host.parse::<IpAddr>().ok().filter(|ip| !ip.is_loopback())
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

fn main() {
    // The host parses a single flag of its own: `--loopback-only` (rejects
    // any declared non-loopback `grpc`, for CI/paranoia). The rest of the
    // command line goes through to the engine guest as-is.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let solo_loopback = args.iter().any(|a| a == "--loopback-only");
    // `--bridge` serves the engine running in a browser instead of running one
    // here (ADR-0030). Like `--loopback-only`, it is the host's own flag and is
    // filtered out before the rest reaches the engine.
    let modo_puente = args.iter().any(|a| a == "--bridge");
    let args_motor: Vec<String> = args
        .iter()
        .filter(|a| *a != "--loopback-only" && *a != "--bridge")
        .cloned()
        .collect();

    // M5-ext.1/2: read the YAML to collect the declared `executors:`. The
    // path is the first **positional** argument (M5, RF-40: the CLI accepts
    // flags before the sequence); if it is missing, or only help was asked
    // for, there is nothing to pre-scan and the engine guest takes over.
    let mut ips_no_loopback: HashSet<IpAddr> = HashSet::new();
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
    let overrides_motor: Vec<String> = Vec::new();
    // How many arguments came from the user. Everything appended past this
    // point is synthetic — the `--executor` overrides — and in bridge mode only
    // those travel: the engine in the browser supplies the
    // sequence itself, and sending the user's positional too would hand it two.
    let args_del_usuario = args_motor.len();
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
    // false: the guest loads the sequence, nobody started its executors, and
    // the user sees `connection-refused` without a single line saying why. The
    // host does not predict the guest's verdict.
    // ADR-0046: nothing is brought up here any more. An executor is an
    // address, the sequence says where, and whoever runs a bench starts it —
    // at boot, by a service manager, or by hand for the examples. The host's
    // spawn path, its dedup by path, its readiness polling and the synthetic
    // `--executor` overrides all went with `type: wasm`.
    //
    // `--executor name=host:port` stays, and it is now the only way the
    // engine's executor table is ever rewritten: by whoever typed it.
    for o in &overrides_motor {
        args_motor_final.push("--executor".into());
        args_motor_final.push(o.clone());
    }

    // --- Bridge mode: everything above already happened — the declared
    // --- executors are up — and instead of running the engine here, we serve
    // --- the one in the browser (ADR-0030). The engine is the same component
    // --- either way (ADR-0031); only its host differs.
    if modo_puente {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) => {
                eprintln!("bridge: could not bind loopback: {e}");
                std::process::exit(1);
            }
        };
        let puerto = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let token = bridge::mint_token();

        // Printed for a person to copy, because that is how the editor gets it:
        // from the terminal that started this. Loopback only, and the URL never
        // leaves the machine.
        println!("bridge listening on 127.0.0.1:{puerto}");
        println!("open the editor with:");
        println!("  http://localhost:5180/?bridge=ws://127.0.0.1:{puerto}/?token={token}");

        // The engine gets its arguments over the wire instead of through argv:
        // there is no argv to inject into when it runs in a browser. These are
        // the same synthetic `--executor` overrides the native path builds
        // above; without them the engine reaches none of the wasm executors.
        let politica = bridge::Policy::new(ips_no_loopback);
        if let Err(e) = bridge::serve(
            listener,
            &token,
            politica,
            &args_motor_final[args_del_usuario..],
        ) {
            eprintln!("bridge: {e}");
            std::process::exit(1);
        }
        return;
    }

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
    // Issue #40: an absolute path argument (`--json /tmp/x.json`, or the
    // sequence itself via `$PWD/...`) falls outside the cwd preopen above
    // (guest name `"."`), so it must get its own preopen, named after its
    // own parent directory, for the guest's unmodified argv string to
    // resolve. A directory that does not exist cannot be preopened (ambient
    // open fails) — that failure surfaces later as the guest's own I/O
    // error, same as today.
    let mut preopens_extra: HashSet<PathBuf> = HashSet::new();
    for ruta in rutas_de_argumentos(&argv[1..]) {
        let p = Path::new(&ruta);
        if p.is_absolute() {
            if let Some(padre) = p.parent() {
                preopens_extra.insert(padre.to_path_buf());
            }
        }
    }
    for dir in &preopens_extra {
        let _ = wasi.preopened_dir(
            dir,
            dir.to_string_lossy(),
            DirPerms::all(),
            FilePerms::all(),
        );
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
                // `{e:?}` and not `{e}`: for a wasmtime error the Display form
                // prints the wasm backtrace and stops, and the chain of causes
                // — what actually trapped — is only in the Debug form. On the
                // windows-latest runner that left a trap in `blocking_read`
                // with no reason attached.
                eprintln!("el motor falló: {e:?}");
                1
            }
        }
    };

    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::{ruta_de_secuencia, rutas_de_argumentos};

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
    fn rutas_de_argumentos_recoge_secuencia_y_flags_de_ruta() {
        let a = args(&[
            "s.yaml",
            "--json",
            "o.json",
            "--csv",
            "o.csv",
            "--limits",
            "l.yaml",
            "--process-model",
            "pm.yaml",
        ]);
        let mut r = rutas_de_argumentos(&a);
        r.sort();
        let mut esperado = vec!["l.yaml", "o.csv", "o.json", "pm.yaml", "s.yaml"];
        esperado.sort();
        assert_eq!(r, esperado);
    }

    #[test]
    fn rutas_de_argumentos_ignora_flags_sin_ruta() {
        // `--executor` takes a value too, but it is not a path: it must not end
        // up preopened as one.
        let a = args(&["s.yaml", "--executor", "n=127.0.0.1:1"]);
        assert_eq!(rutas_de_argumentos(&a), vec!["s.yaml".to_string()]);
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
}
