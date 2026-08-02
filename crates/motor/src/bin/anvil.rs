//! CLI headless de anvil (M5, RF-40): lee una secuencia desde YAML, la
//! traduce a un `Programa` y la corre contra el ejecutor de pasos por gRPC.
//! Es el CLI maduro del roadmap:
//!   ./anvil <secuencia.yaml> [opciones]
//!
//! Opciones:
//!   --process-model <ruta>  envuelve la secuencia en un process model (RF-38)
//!   --json <ruta>           vuelca el reporte a JSON
//!   --csv <ruta>            vuelca el reporte a CSV
//!   --limits <ruta>         sidecar de límites (RF-30, inyecta por nombre de paso)
//!   --port <n>              puerto del ejecutor (default 9100)
//!   --validate              carga y valida sin ejecutar ni conectar (dry-run)
//!   --quiet                 silencia el reporte de consola y los logs stderr
//!   --help / --version      ayuda y versión
//!
//! El resultado se vierte a `ResultSink`s (M2): consola salvo `--quiet`, y
//! opcionalmente JSON/CSV en fichero. Los logs de arranque van a stderr para
//! dejar stdout limpio para el sink de consola. Parseo manual, sin clap: el
//! flag set es pequeño y se evita peso en el `.wasm` (ADR-0001).
//!
//! En el binario único (ADR-0011) el host hereda estos args al guest motor y
//! ya espera al ejecutor; los flags fluyen solos. `--port` es útil para dos
//! terminales (wasmtime) o un ejecutor externo.

use cargador::{
    aplicar_limites, aplicar_override_ejecutores, cargar_limites_de_archivo,
    cargar_programa_con_pm, cargar_programa_de_archivo,
};
use modelo::{Programa, ResultSink};
use motor::Motor;
use result_sink::{SinkConsola, SinkCsv, SinkJson};
use std::fs::File;
use std::thread;
use std::time::Duration;

const PUERTO_DEFAULT: u16 = 9100;
/// Reintentos de conexión al ejecutor × `ESPERA_MS` = 5 s máx, como
/// `esperar_ejecutor` del host. Sin backoff exponencial (post-MVP).
const REINTENTOS_CONEX: u32 = 500;
const ESPERA_MS: u64 = 10;

/// Acción de salida anticipada del parser: `--help`/`--version` (exit 0) o
/// un error de uso (exit 2). Separado de `Cli` para que el parser sea puro
/// y testeable sin tocar `process::exit`.
#[derive(Debug)]
enum AccionEarlyExit {
    Help,
    Version,
    Uso(String),
}

/// Configuración parseada desde la línea de comandos.
struct Cli {
    ruta: String,
    process_model: Option<String>,
    json: Option<String>,
    csv: Option<String>,
    limits: Option<String>,
    /// Overrides `nombre=host:puerto` de `--ejecutor` (RF-36.3), acumulables.
    ejecutores: Vec<String>,
    port: u16,
    validate: bool,
    quiet: bool,
}

fn usage() {
    eprintln!(
        "uso: anvil <secuencia.yaml> [opciones]\n\nOpciones:\n  \
--process-model <ruta>  envuelve la secuencia en un process model (RF-38)\n  \
--json <ruta>           vuelca el reporte a JSON\n  \
--csv <ruta>            vuelca el reporte a CSV\n  \
--limits <ruta>         sidecar de límites (RF-30)\n  \
--ejecutor n=host:puerto  re-apunta un ejecutor declarado (RF-36.3)\n  \
--port <n>              puerto del ejecutor embebido (default {PUERTO_DEFAULT})\n  \
--validate              carga y valida sin ejecutar ni conectar\n  \
--quiet                 silencia el reporte de consola y logs stderr\n  \
--help                  muestra esta ayuda\n  \
--version               muestra la versión"
    );
}

/// Parser puro de la línea de comandos. `--help`/`--version` pueden aparecer
/// en cualquier posición y se atajan con `Err(AccionEarlyExit::...)`. La
/// secuencia es el primer argumento posicional (no-flag); los flags que
/// toman valor se consumen en el orden dado.
fn parse_cli(args: Vec<String>) -> Result<Cli, AccionEarlyExit> {
    let mut cli = Cli {
        ruta: String::new(),
        process_model: None,
        json: None,
        csv: None,
        limits: None,
        ejecutores: Vec::new(),
        port: PUERTO_DEFAULT,
        validate: false,
        quiet: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => return Err(AccionEarlyExit::Help),
            "--version" => return Err(AccionEarlyExit::Version),
            "--validate" => cli.validate = true,
            "--quiet" => cli.quiet = true,
            "--process-model" | "--json" | "--csv" | "--limits" | "--ejecutor" | "--port" => {
                let valor = match args.next() {
                    Some(v) => v,
                    None => {
                        return Err(AccionEarlyExit::Uso(format!(
                            "el flag '{arg}' necesita un valor"
                        )));
                    }
                };
                match arg.as_str() {
                    "--process-model" => cli.process_model = Some(valor),
                    "--json" => cli.json = Some(valor),
                    "--csv" => cli.csv = Some(valor),
                    "--limits" => cli.limits = Some(valor),
                    "--ejecutor" => cli.ejecutores.push(valor),
                    "--port" => {
                        cli.port = valor.parse().map_err(|_| {
                            AccionEarlyExit::Uso(format!("--port inválido: {valor}"))
                        })?;
                    }
                    _ => unreachable!(),
                }
            }
            other if other.starts_with("--") => {
                return Err(AccionEarlyExit::Uso(format!("flag desconocido: '{other}'")));
            }
            other => {
                if cli.ruta.is_empty() {
                    cli.ruta = other.to_string();
                } else {
                    return Err(AccionEarlyExit::Uso(format!(
                        "argumento de más: '{other}'"
                    )));
                }
            }
        }
    }
    if cli.ruta.is_empty() {
        return Err(AccionEarlyExit::Uso("falta la secuencia YAML".into()));
    }
    Ok(cli)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_cli(args) {
        Ok(c) => c,
        Err(AccionEarlyExit::Help) => {
            usage();
            std::process::exit(0);
        }
        Err(AccionEarlyExit::Version) => {
            eprintln!("anvil {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        Err(AccionEarlyExit::Uso(m)) => {
            eprintln!("{m}");
            usage();
            std::process::exit(2);
        }
    };

    // Carga (+ PM si se pide). Fall-fast antes de tocar el ejecutor.
    let mut programa = match cargar(&cli) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no se pudo cargar la secuencia '{ruta}': {e}", ruta = cli.ruta);
            std::process::exit(1);
        }
    };
    if !cli.quiet {
        eprintln!(
            "secuencia '{}' cargada ({} pasos en main, {} subsecuencia(s) externa(s), {} ejecutor(es))",
            programa.raiz.nombre,
            programa.raiz.pasos_main.len(),
            programa.archivos.len(),
            programa.ejecutores.len()
        );
    }

    // Override de ejecutores (RF-36.3, patrón --limits): re-apunta un
    // ejecutor a otro endpoint sin tocar el YAML (R&D vs. fábrica).
    if !cli.ejecutores.is_empty() {
        match aplicar_override_ejecutores(&mut programa, &cli.ejecutores) {
            Ok(n) => {
                if !cli.quiet {
                    eprintln!("override de ejecutores aplicado ({n} afectado(s))");
                }
            }
            Err(e) => {
                eprintln!("no se pudo aplicar el override de ejecutores: {e}");
                std::process::exit(1);
            }
        }
    }

    // Property loader (RF-30): sidecar de límites opcional, inyectado por
    // nombre de paso en la **raíz**, sobreescribiendo los embebidos. (Aplicar
    // el sidecar a las subsecuencias externas es post-MVP.)
    if let Some(r) = cli.limits.as_deref() {
        match cargar_limites_de_archivo(r) {
            Ok(l) => {
                let n = aplicar_limites(&mut programa.raiz, &l);
                if !cli.quiet {
                    eprintln!("sidecar de límites '{r}' aplicado ({n} paso(s) afectado(s))");
                }
            }
            Err(e) => {
                eprintln!("no se pudo cargar el sidecar de límites '{r}': {e}");
                std::process::exit(1);
            }
        }
    }

    // --validate: carga + valida (cargador + sidecar) sin ejecutar ni
    // conectar. Útil en CI sin hardware.
    if cli.validate {
        if !cli.quiet {
            eprintln!(
                "'{}' válida ({} paso(s) en main, {} subsecuencia(s) externa(s))",
                programa.raiz.nombre,
                programa.raiz.pasos_main.len(),
                programa.archivos.len()
            );
        }
        std::process::exit(0);
    }

    // Abrir los ficheros de salida antes de conectar el motor: si una ruta
    // no se puede crear, fallamos sin haber tocado el ejecutor.
    let mut json = abrir_sink(&cli.json, "JSON", SinkJson::nuevo);
    let mut csv = abrir_sink(&cli.csv, "CSV", SinkCsv::nuevo);

    // Conexión con reintento (RF-40) al ejecutor embebido + una conexión por
    // ejecutor `grpc` declarado (M5-ext.1, RF-36.3). `Motor::conecta` caía al
    // primer intento si el ejecutor no estaba listo; el host embebido ya
    // espera, así que esto cubre dos terminales y arranques lentos.
    let mut motor = match conecta_con_reintento(&programa, "127.0.0.1", cli.port, cli.quiet) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("no se pudo conectar a los ejecutores de pasos: {e}");
            std::process::exit(1);
        }
    };
    if !cli.quiet {
        eprintln!("conectado a los ejecutores de pasos (embebido en 127.0.0.1:{})", cli.port);
    }

    // Sinks: consola salvo --quiet; JSON/CSV si se piden. El composite los
    // agrupa como un único sink para el motor.
    let mut consola = if cli.quiet {
        None
    } else {
        Some(SinkConsola::nuevo(std::io::stdout()))
    };
    let mut sinks: Vec<&mut dyn ResultSink> = Vec::new();
    if let Some(c) = consola.as_mut() {
        sinks.push(c);
    }
    if let Some(j) = json.as_mut() {
        sinks.push(j);
    }
    if let Some(c) = csv.as_mut() {
        sinks.push(c);
    }
    let mut composite = modelo::SinkCompuesto::nuevo(sinks);

    if let Err(e) = motor.ejecuta_programa(&programa, &mut composite) {
        eprintln!("la secuencia se interrumpió: {e}");
        std::process::exit(1);
    }
}

/// Carga el programa: con PM si `--process-model`, sin él en caso contrario
/// (comportamiento actual, la secuencia corre tal cual).
fn cargar(cli: &Cli) -> Result<Programa, cargador::ErrorCarga> {
    if let Some(pm) = cli.process_model.as_deref() {
        cargar_programa_con_pm(pm, &cli.ruta)
    } else {
        cargar_programa_de_archivo(&cli.ruta)
    }
}

/// Reintenta `Motor::desde_programa_en` hasta `REINTENTOS_CONEX × ESPERA_MS`
/// (5 s), como `esperar_ejecutor` del host. Sin backoff exponencial (post-MVP).
fn conecta_con_reintento(
    programa: &Programa,
    host: &str,
    port: u16,
    quiet: bool,
) -> Result<Motor, motor::Error> {
    let mut ultimo: Option<motor::Error> = None;
    for i in 0..REINTENTOS_CONEX {
        match Motor::desde_programa_en(programa, host, port) {
            Ok(m) => return Ok(m),
            Err(e) => {
                if !quiet && i == 0 {
                    eprintln!("esperando al ejecutor en {host}:{port}…");
                }
                ultimo = Some(e);
                thread::sleep(Duration::from_millis(ESPERA_MS));
            }
        }
    }
    Err(ultimo.expect("el bucle hace al menos un intento"))
}

/// Abre un fichero de sink si se pidió; sale (exit 1) si no se puede crear.
fn abrir_sink<T>(ruta: &Option<String>, kind: &str, ctor: fn(File) -> T) -> Option<T> {
    ruta.as_deref().map(|r| match File::create(r) {
        Ok(f) => ctor(f),
        Err(e) => {
            eprintln!("no se pudo abrir '{r}' para {kind}: {e}");
            std::process::exit(1);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_es_early_exit() {
        assert!(matches!(parse_cli(vec!["--help".into()]), Err(AccionEarlyExit::Help)));
    }

    #[test]
    fn parse_version_es_early_exit() {
        assert!(matches!(parse_cli(vec!["--version".into()]), Err(AccionEarlyExit::Version)));
    }

    #[test]
    fn parse_help_antes_de_la_ruta_sigue_siendo_early_exit() {
        assert!(matches!(
            parse_cli(vec!["--help".into(), "s.yaml".into()]),
            Err(AccionEarlyExit::Help)
        ));
    }

    #[test]
    fn parse_ruta_y_flags_de_salida() {
        let c = parse_cli(vec!["s.yaml".into(), "--json".into(), "o.json".into(), "--csv".into(), "o.csv".into()]).unwrap();
        assert_eq!(c.ruta, "s.yaml");
        assert_eq!(c.json.as_deref(), Some("o.json"));
        assert_eq!(c.csv.as_deref(), Some("o.csv"));
        assert!(!c.validate);
        assert!(!c.quiet);
        assert_eq!(c.port, PUERTO_DEFAULT);
    }

    #[test]
    fn parse_validate_y_quiet_son_booleanos() {
        let c = parse_cli(vec!["s.yaml".into(), "--validate".into(), "--quiet".into()]).unwrap();
        assert!(c.validate);
        assert!(c.quiet);
    }

    #[test]
    fn parse_port() {
        let c = parse_cli(vec!["s.yaml".into(), "--port".into(), "1234".into()]).unwrap();
        assert_eq!(c.port, 1234);
    }

    #[test]
    fn parse_port_invalido_es_uso() {
        assert!(matches!(
            parse_cli(vec!["s.yaml".into(), "--port".into(), "noes".into()]),
            Err(AccionEarlyExit::Uso(_))
        ));
    }

    #[test]
    fn parse_process_model() {
        let c = parse_cli(vec!["s.yaml".into(), "--process-model".into(), "pm.yaml".into()]).unwrap();
        assert_eq!(c.process_model.as_deref(), Some("pm.yaml"));
    }

    #[test]
    fn parse_flag_sin_valor_es_uso() {
        assert!(matches!(
            parse_cli(vec!["s.yaml".into(), "--json".into()]),
            Err(AccionEarlyExit::Uso(_))
        ));
    }

    #[test]
    fn parse_flag_desconocido_es_uso() {
        assert!(matches!(
            parse_cli(vec!["s.yaml".into(), "--inventado".into()]),
            Err(AccionEarlyExit::Uso(_))
        ));
    }

    #[test]
    fn parse_sin_ruta_es_uso() {
        assert!(matches!(parse_cli(vec![]), Err(AccionEarlyExit::Uso(_))));
    }

    #[test]
    fn parse_argumento_de_mas_es_uso() {
        assert!(matches!(
            parse_cli(vec!["s.yaml".into(), "otra.yaml".into()]),
            Err(AccionEarlyExit::Uso(_))
        ));
    }
}