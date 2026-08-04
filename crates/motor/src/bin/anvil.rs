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
    cargar_programa_con_process_model, cargar_programa_de_archivo,
};
use modelo::ResultSink;
use motor::Motor;
use result_sink::{SinkConsola, SinkCsv, SinkJson};
use std::fs::File;

const USO: &str = "\
uso: anvil <secuencia.yaml> [opciones]

Corre una secuencia de test contra el ejecutor de pasos (gRPC en loopback).

Argumentos posicionales:
  <secuencia.yaml>            La secuencia a ejecutar (la del operador).

Opciones:
  --process-model <pm.yaml>   Corre la secuencia envuelta en un process model
                              (M5/RF-38): el PM es la raíz, identifica el UUT,
                              invoca la secuencia del operador y notifica.
  --json <ruta>               Vierte el resultado también a JSON.
  --csv <ruta>                Vierte el resultado también a CSV.
  --limits <ruta>             Sidecar de límites (property loader, RF-30).
  --ejecutor nombre=host:puerto
                              Re-apunta un ejecutor a otro endpoint (RF-36.3).
  -h, --help                  Muestra esta ayuda y sale.
  -V, --version               Muestra la versión y sale.

Nota: --solo-loopback es un flag del binario 'anvil' (host embebido), no de
este guest; rechaza ejecutores en IPs no-loopback (CI/paranoia, ADR-0011).";

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // --help/--version en cualquier posición: se atienden antes de validar
    // el resto (un `anvil --help` sin secuencia no debe fallar).
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USO}");
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("anvil {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    // La secuencia es el primer argumento **posicional** (los flags pueden
    // ir antes o después, p. ej. `anvil --process-model pm.yaml s.yaml`).
    // Los flags con valor se saltan junto a su valor para no confundirlo
    // con un posicional.
    let flags_con_valor = ["--process-model", "--json", "--csv", "--limits", "--ejecutor"];
    let mut ruta: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if flags_con_valor.contains(&args[i].as_str()) {
            i += 2;
        } else if args[i].starts_with("--") {
            i += 1;
        } else {
            ruta = Some(args.remove(i));
            break;
        }
    }
    let ruta = match ruta {
        Some(r) => r,
        None => {
            eprintln!("{USO}");
            std::process::exit(2);
        }
    };

    // Parseo manual de flags (sin clap, como el resto del CLI). Acepta
    // --process-model/--json/--csv/--limits/--ejecutor <ruta> en cualquier
    // orden tras la secuencia.
    let mut process_model_ruta: Option<String> = None;
    let mut json_ruta: Option<String> = None;
    let mut csv_ruta: Option<String> = None;
    let mut limits_ruta: Option<String> = None;
    let mut overrides_ejecutores: Vec<String> = Vec::new();
    let mut it = args.into_iter();
    while let Some(flag) = it.next() {
        let valor = it.next().unwrap_or_else(|| {
            eprintln!("el flag '{flag}' necesita un valor");
            eprintln!("{USO}");
            std::process::exit(2);
        });
        match flag.as_str() {
            "--process-model" => process_model_ruta = Some(valor),
            "--json" => json_ruta = Some(valor),
            "--csv" => csv_ruta = Some(valor),
            "--limits" => limits_ruta = Some(valor),
            "--ejecutor" => overrides_ejecutores.push(valor),
            other => {
                eprintln!("flag desconocido: '{other}'");
                eprintln!("{USO}");
                std::process::exit(2);
            }
        }
    }

    // M5 (RF-38): con --process-model, el PM es la raíz y la secuencia del
    // operador se inyecta como subsecuencia usuario (ADR-0016). Sin él, la
    // secuencia es la raíz (compat M5-ext.2).
    let mut programa = match process_model_ruta.as_deref() {
        Some(pm) => match cargar_programa_con_process_model(pm, &ruta) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("no se pudo cargar el process model '{pm}' con la secuencia '{ruta}': {e}");
                std::process::exit(1);
            }
        },
        None => match cargar_programa_de_archivo(&ruta) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("no se pudo cargar la secuencia '{ruta}': {e}");
                std::process::exit(1);
            }
        },
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
