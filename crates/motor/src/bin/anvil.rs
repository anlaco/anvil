//! CLI de anvil: lee una secuencia desde YAML, la traduce a
//! `DefinicionSecuencia` y la corre contra el ejecutor de pasos por gRPC.
//! Es la semilla del CLI headless del roadmap (M5:
//! `wasmtime run anvil.wasm secuencia.yaml`).
//!
//! Correr (desde la raíz del repo), con el ejecutor ya levantado en otra
//! terminal (ver `basica_datos.rs`):
//!   wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
//!     target/wasm32-wasip2/debug/anvil.wasm ejemplos/basica.yaml \
//!     --json /tmp/out.json --csv /tmp/out.csv
//!
//! `--dir=.` expone el directorio actual al WASM para que pueda leer el
//! YAML y escribir los ficheros de salida; las rutas van como argumentos.
//!
//! El resultado se vierte a `ResultSink`s (M2): **siempre** a consola (el
//! formato textual congelado, RNF-08) y, si se pide, también a JSON y/o
//! CSV en fichero. Los logs de arranque van a stderr para dejar stdout
//! limpio para el sink de consola.

use cargador::{
    aplicar_limites, aplicar_override_ejecutores, cargar_limites_de_archivo,
    cargar_programa_de_archivo,
};
use modelo::ResultSink;
use motor::Motor;
use result_sink::{SinkConsola, SinkCsv, SinkJson};
use std::fs::File;

fn main() {
    let mut args = std::env::args().skip(1);
    let ruta = match args.next() {
        Some(r) => r,
        None => {
            eprintln!("uso: anvil <secuencia.yaml> [--json <ruta>] [--csv <ruta>] [--limits <ruta>] [--ejecutor nombre=host:puerto]");
            std::process::exit(2);
        }
    };

    // Parseo manual de flags (sin clap, como el resto del CLI). Acepta
    // --json/--csv/--limits/--ejecutor <ruta> en cualquier orden tras la
    // secuencia.
    let mut json_ruta: Option<String> = None;
    let mut csv_ruta: Option<String> = None;
    let mut limits_ruta: Option<String> = None;
    let mut overrides_ejecutores: Vec<String> = Vec::new();
    while let Some(flag) = args.next() {
        let valor = args.next().unwrap_or_else(|| {
            eprintln!("el flag '{flag}' necesita una ruta");
            std::process::exit(2);
        });
        match flag.as_str() {
            "--json" => json_ruta = Some(valor),
            "--csv" => csv_ruta = Some(valor),
            "--limits" => limits_ruta = Some(valor),
            "--ejecutor" => overrides_ejecutores.push(valor),
            other => {
                eprintln!("flag desconocido: '{other}'");
                std::process::exit(2);
            }
        }
    }

    let mut programa = match cargar_programa_de_archivo(&ruta) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no se pudo cargar la secuencia '{ruta}': {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "secuencia '{}' cargada ({} pasos, {} subsecuencia(s) externa(s), {} ejecutor(es))",
        programa.raiz.nombre,
        programa.raiz.pasos_main.len(),
        programa.archivos.len(),
        programa.ejecutores.len()
    );

    // Override de ejecutores (RF-36.3, patrón --limits): re-apunta un
    // ejecutor a otro endpoint sin tocar el YAML (R&D vs. fábrica).
    if !overrides_ejecutores.is_empty() {
        match aplicar_override_ejecutores(&mut programa, &overrides_ejecutores) {
            Ok(n) => eprintln!("override de ejecutores aplicado ({n} afectado(s))"),
            Err(e) => {
                eprintln!("no se pudo aplicar el override de ejecutores: {e}");
                std::process::exit(1);
            }
        }
    }

    // Property loader (RF-30): si se pide un sidecar de límites, se inyecta
    // por nombre de paso en la **raíz**, sobreescribiendo los límites
    // embebidos. (Aplicar el sidecar a las subsecuencias externas es
    // post-MVP; hoy el sidecar cubre la secuencia principal.)
    if let Some(r) = limits_ruta.as_deref() {
        let limites = match cargar_limites_de_archivo(r) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("no se pudo cargar el sidecar de límites '{r}': {e}");
                std::process::exit(1);
            }
        };
        let n = aplicar_limites(&mut programa.raiz, &limites);
        eprintln!("sidecar de límites '{r}' aplicado ({n} paso(s) afectado(s))");
    }

    // Abrir los ficheros de salida antes de conectar el motor: si una ruta
    // no se puede crear, fallamos sin haber tocado el ejecutor.
    let mut json = match json_ruta.as_deref() {
        Some(r) => match File::create(r) {
            Ok(f) => Some(SinkJson::nuevo(f)),
            Err(e) => {
                eprintln!("no se pudo abrir '{r}' para JSON: {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let mut csv = match csv_ruta.as_deref() {
        Some(r) => match File::create(r) {
            Ok(f) => Some(SinkCsv::nuevo(f)),
            Err(e) => {
                eprintln!("no se pudo abrir '{r}' para CSV: {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    let mut motor = match Motor::desde_programa(&programa) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("no se pudo conectar a los ejecutores de pasos: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("conectado a los ejecutores de pasos");

    // Consola siempre; JSON/CSV si se pidieron. El composite los agrupa
    // como un único sink para el motor.
    let mut consola = SinkConsola::nuevo(std::io::stdout());
    let mut sinks: Vec<&mut dyn ResultSink> = vec![&mut consola];
    if let Some(j) = json.as_mut() {
        sinks.push(j);
    }
    if let Some(c) = csv.as_mut() {
        sinks.push(c);
    }
    let mut composite = modelo::SinkCompuesto::nuevo(sinks);

    match motor.ejecuta_programa(&programa, &mut composite) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("la secuencia se interrumpió: {e}");
            std::process::exit(1);
        }
    }
}