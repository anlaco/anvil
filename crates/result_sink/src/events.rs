//! Sink de eventos: una línea JSON por cosa que pasa, mientras pasa
//! (ADR-0029, ADR-0033). Es el único sink que escribe **durante** la
//! ejecución; los tres de formato esperan al agregado final.
//!
//! Lo que hace honesto a este canal, y por qué:
//!
//! - **La identidad es la ejecución, no el paso.** `step_run_id` y
//!   `parent_run_id` los acuña el motor, que es el único que sabe qué
//!   invocación es esta. El nombre y la ruta viajan como descripción: el
//!   cargador no exige nombres únicos, así que indexar por nombre ilumina
//!   la fila equivocada (ADR-0033 §1).
//! - **La paternidad se afirma en cada línea**, no se reconstruye con una
//!   pila: perder una línea cuesta un nodo, no el árbol (§2).
//! - **`seq` se asigna antes de intentar la escritura**, para que una línea
//!   descartada deje hueco. Al revés, el hueco desaparece y con él la única
//!   forma que tiene un lector de saber que perdió algo (§4b).
//! - **El sink se traga sus errores de IO.** Avisar con `eprintln!` desde un
//!   sink cuyo destino *es* stderr entra en pánico si esa escritura también
//!   falla, y mata la secuencia sin cleanup.

use modelo::{DefinicionPaso, DefinicionSecuencia, Fase, IdentidadPaso, ResultSink};
use modelo::{ResultadoSecuencia, ResultadoStep};
use serde_json::{json, Map, Value};
use std::io::Write;

use crate::json::paso_a_json_con;

/// Versión del cable. Sube **sólo** cuando cambia el significado de un
/// evento o de un campo de forma que un lector conforme no puede absorber;
/// añadir claves es compatible hacia atrás (ADR-0029 §5), y para eso no se
/// toca. No es la versión del motor, que se mueve por otras razones.
pub const EVENTS_VERSION: u32 = 1;

/// Vierte la ejecución como NDJSON, un objeto por línea.
pub struct SinkEventos<W: Write> {
    salida: W,
    run_id: String,
    seq: u64,
}

impl<W: Write> SinkEventos<W> {
    pub fn nuevo(salida: W, run_id: impl Into<String>) -> Self {
        SinkEventos {
            salida,
            run_id: run_id.into(),
            seq: 0,
        }
    }

    /// Escribe una línea. El `seq` se gasta **aunque la escritura falle**:
    /// es lo que convierte una pérdida silenciosa en un hueco visible.
    fn emite(&mut self, evento: &str, campos: Map<String, Value>) {
        let seq = self.seq;
        self.seq += 1;
        let mut linea = Map::new();
        linea.insert("event".into(), json!(evento));
        for (k, v) in campos {
            linea.insert(k, v);
        }
        linea.insert("run_id".into(), json!(self.run_id));
        linea.insert("seq".into(), json!(seq));
        let mut texto = match serde_json::to_string(&Value::Object(linea)) {
            Ok(t) => t,
            Err(_) => return,
        };
        texto.push('\n');
        // Una sola escritura, salto incluido: `writeln!` la parte en dos
        // llamadas, y fd 2 lo comparten el motor y los
        // ejecutores lanzados. Otro escritor que se cuele entre el contenido
        // y el salto convierte dos líneas válidas en una rota.
        //
        // El error se traga y no se latcha. Tragarlo, porque avisar con
        // `eprintln!` desde un sink cuyo destino *es* stderr entra en pánico
        // si esa escritura también falla, y mata la secuencia sin cleanup.
        // No latcharlo, porque un fallo transitorio dejaría el flujo mudo
        // para siempre: escribir a un destino muerto falla al instante, así
        // que reintentar no cuesta nada, y así una pérdida deja **hueco** en
        // vez de truncar (ADR-0033 §4b).
        let _ = self.salida.write_all(texto.as_bytes());
    }

    /// Los campos que comparten las tres líneas de un paso.
    fn identidad(&self, id: &IdentidadPaso) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("step_run_id".into(), json!(id.step_run_id));
        m.insert(
            "parent_run_id".into(),
            match id.parent_run_id {
                Some(p) => json!(p),
                None => Value::Null,
            },
        );
        m.insert("depth".into(), json!(id.depth));
        m.insert("phase".into(), json!(id.fase.como_texto()));
        m.insert("locator".into(), locator(id));
        m
    }
}

/// Dónde está el paso en el programa **tal como se cargó**. Es una pista,
/// no una clave: la misma posición señala otro acto tras cualquier edición,
/// y bajo `--process-model` el mismo acto está en otra posición.
///
/// Sin `source`: `DefinicionSecuencia` no lleva la ruta de su fichero, así
/// que hoy no hay de dónde sacarla. Es trabajo pendiente (#60), y hasta
/// entonces `sequence` es lo único que sitúa la ruta.
fn locator(id: &IdentidadPaso) -> Value {
    let path: Vec<Value> = id
        .path
        .iter()
        .flat_map(|(f, i)| [json!(f.como_texto()), json!(i)])
        .collect();
    json!({ "sequence": id.sequence, "path": path })
}

/// Los pasos que la secuencia **declara**, antes de correr nada.
///
/// Es el denominador. Un paso que la regla del primer fallo corta no emite
/// ninguna línea, y un hueco en `seq` necesita línea a ambos lados —así que
/// una pérdida en la cola no deja hueco—: sin esto, un lector no puede
/// distinguir «no llegó a correr» de «perdí sus líneas» (ADR-0033 §4b).
fn plan(def: &DefinicionSecuencia) -> Value {
    let mut pasos = Vec::new();
    for (fase, lista) in [
        (Fase::Setup, &def.pasos_setup),
        (Fase::Main, &def.pasos_main),
        (Fase::Cleanup, &def.pasos_cleanup),
    ] {
        for (i, p) in lista.iter().enumerate() {
            pasos.push(json!({
                "name": p.nombre,
                "phase": fase.como_texto(),
                "index": i,
            }));
        }
    }
    Value::Array(pasos)
}

impl<W: Write> ResultSink for SinkEventos<W> {
    fn on_inicio_secuencia(&mut self, secuencia: &DefinicionSecuencia) {
        let mut m = Map::new();
        m.insert("sequence".into(), json!(secuencia.nombre));
        m.insert("events_version".into(), json!(EVENTS_VERSION));
        m.insert("plan".into(), plan(secuencia));
        self.emite("sequence_start", m);
    }

    fn on_inicio_paso(&mut self, paso: &DefinicionPaso, id: &IdentidadPaso) {
        let mut m = Map::new();
        m.insert("name".into(), json!(paso.nombre));
        for (k, v) in self.identidad(id) {
            m.insert(k, v);
        }
        self.emite("step_start", m);
    }

    fn on_resultado(&mut self, resultado: &ResultadoStep, id: &IdentidadPaso) {
        // El objeto del informe, **plano**, para que la vista en vivo y el
        // informe no puedan discrepar sobre una medida: sale del mismo
        // código, no de una segunda escritura (ADR-0029 §Consecuencias).
        let payload = paso_a_json_con(resultado, false);
        let mut m = Map::new();
        let mut cola = Map::new();
        if let Value::Object(obj) = payload {
            for (k, v) in obj {
                // `inputs`/`outputs` van al final: lo primero de la línea
                // tiene que ser lo que una persona lee (§3).
                if k == "inputs" || k == "outputs" {
                    cola.insert(k, v);
                } else if k != "phase" {
                    // `phase` ya viaja en la identidad.
                    m.insert(k, v);
                }
            }
        }
        for (k, v) in self.identidad(id) {
            m.insert(k, v);
        }
        // La ausencia de `sub_steps` NO significa «sin hijos»: los hijos ya
        // viajaron como líneas propias. `has_children` lo dice explícitamente
        // porque el objeto es por lo demás idéntico al del informe, y un
        // lector que reutilice ese parser vería una hoja.
        let hijos = resultado.sub_pasos.as_ref().is_some_and(|v| !v.is_empty());
        m.insert("has_children".into(), json!(hijos));
        // Hoy siempre `true`: un paso produce exactamente un `on_resultado`,
        // y los reintentos ocurren por debajo del sink. Se emite igual para
        // que un lector que filtre por él siga funcionando el día que se
        // emitan intentos superados.
        m.insert("final".into(), json!(true));
        for (k, v) in cola {
            m.insert(k, v);
        }
        self.emite("step_result", m);
    }

    fn on_fin_paso(&mut self, paso: &DefinicionPaso, id: &IdentidadPaso) {
        let mut m = Map::new();
        m.insert("name".into(), json!(paso.nombre));
        for (k, v) in self.identidad(id) {
            m.insert(k, v);
        }
        self.emite("step_end", m);
    }

    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        let (saltados, total) = secuencia.saltados();
        let mut m = Map::new();
        m.insert("sequence".into(), json!(secuencia.nombre));
        m.insert("status".into(), json!(secuencia.estado()));
        m.insert("skipped_steps".into(), json!(saltados));
        m.insert("total_steps".into(), json!(total));
        // Cuenta su propia línea, para que un lector sin huecos pueda
        // afirmar que tiene el flujo entero y no sólo esperarlo.
        m.insert("seq_total".into(), json!(self.seq + 1));
        self.emite("sequence_end", m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelo::DefinicionPaso;

    /// Un `Write` que se niega a escribir la línea N. Es la única forma de
    /// ejercer una pérdida de forma determinista: contra un stderr real, el
    /// descarte depende de la carga y no se puede afirmar en un test.
    struct EscritorQueFalla {
        lineas: Vec<String>,
        rechaza: usize,
        n: usize,
    }

    impl Write for EscritorQueFalla {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = self.n;
            self.n += 1;
            if n == self.rechaza {
                return Err(std::io::Error::other("destino cerrado"));
            }
            // Que esto valga es en sí una afirmación: el sink escribe una
            // línea por llamada, salto incluido.
            let t = String::from_utf8_lossy(buf);
            assert!(t.ends_with('\n'), "cada escritura es una línea entera");
            self.lineas.push(t.trim_end().to_string());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn identidad<'a>(
        sid: &'a str,
        padre: Option<&'a str>,
        ruta: &'a [(Fase, usize)],
    ) -> IdentidadPaso<'a> {
        IdentidadPaso {
            step_run_id: sid,
            parent_run_id: padre,
            depth: padre.is_some() as usize,
            fase: Fase::Main,
            sequence: "s",
            path: ruta,
        }
    }

    fn corre(rechaza: usize) -> Vec<serde_json::Value> {
        let escritor = EscritorQueFalla {
            lineas: Vec::new(),
            rechaza,
            n: 0,
        };
        let mut sink = SinkEventos::nuevo(escritor, "run");
        let paso = DefinicionPaso::nuevo("medir", 1);
        let id = identidad("aaa", None, &[(Fase::Main, 0)]);
        sink.on_inicio_paso(&paso, &id);
        let r = ResultadoStep::nuevo("medir", "pass", "ok");
        sink.on_resultado(&r, &id);
        sink.on_fin_paso(&paso, &id);
        sink.salida
            .lineas
            .iter()
            .map(|l| serde_json::from_str(l).expect("cada línea es un JSON completo"))
            .collect()
    }

    #[test]
    fn las_tres_lineas_de_un_paso_comparten_la_identidad() {
        let ls = corre(usize::MAX);
        assert_eq!(ls.len(), 3);
        let ids: Vec<_> = ls.iter().map(|l| l["step_run_id"].as_str()).collect();
        assert!(
            ids.iter().all(|i| *i == ids[0]),
            "step_start, step_result y step_end son el mismo acto: {ids:?}"
        );
        let evs: Vec<_> = ls.iter().map(|l| l["event"].as_str().unwrap()).collect();
        assert_eq!(evs, ["step_start", "step_result", "step_end"]);
    }

    #[test]
    fn una_linea_descartada_deja_hueco_en_seq() {
        let ls = corre(1); // se rechaza la escritura de `step_result`
        let seqs: Vec<u64> = ls.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
        // El 1 falta, y el 2 sigue ahí: eso es el hueco. Sin él un lector no
        // puede saber que perdió algo, y la Regla 2 de ADR-0019 deja de ser
        // honrable — concluiría con confianza desde un flujo incompleto.
        assert_eq!(seqs, vec![0, 2], "la línea perdida deja su número vacío");
    }

    #[test]
    fn el_seq_se_gasta_aunque_la_escritura_falle() {
        // La primera línea se pierde: la siguiente NO puede volver a ser 0.
        let escritor = EscritorQueFalla {
            lineas: Vec::new(),
            rechaza: usize::MAX,
            n: 0,
        };
        let mut sink = SinkEventos::nuevo(escritor, "run");
        sink.seq = 7; // como si ya se hubieran emitido siete
        let paso = DefinicionPaso::nuevo("medir", 1);
        sink.on_inicio_paso(&paso, &identidad("aaa", None, &[(Fase::Main, 0)]));
        let v: serde_json::Value = serde_json::from_str(&sink.salida.lineas[0]).unwrap();
        assert_eq!(v["seq"], 7);
    }

    #[test]
    fn el_padre_viaja_en_cada_linea_del_hijo() {
        let escritor = EscritorQueFalla {
            lineas: Vec::new(),
            rechaza: usize::MAX,
            n: 0,
        };
        let mut sink = SinkEventos::nuevo(escritor, "run");
        let paso = DefinicionPaso::nuevo("hijo", 1);
        let id = identidad("bbb", Some("padre"), &[(Fase::Main, 0), (Fase::Main, 2)]);
        sink.on_inicio_paso(&paso, &id);
        let v: serde_json::Value = serde_json::from_str(&sink.salida.lineas[0]).unwrap();
        assert_eq!(v["parent_run_id"], "padre");
        assert_eq!(v["depth"], 1);
        // La ruta se afirma entera: un lector que se pierda el step_start del
        // padre sigue sabiendo dónde está este paso.
        assert_eq!(
            v["locator"]["path"],
            serde_json::json!(["main", 0, "main", 2])
        );
    }

    #[test]
    fn el_veredicto_va_al_principio_de_la_linea() {
        let ls = corre(usize::MAX);
        let crudo = serde_json::to_string(&ls[1]).unwrap();
        let hasta_status = crudo.find("\"status\"").expect("step_result lleva status");
        // El orden de claves es parte del formato: quien mira un terminal con
        // la línea partida tiene que llegar al veredicto sin leerla entera.
        assert!(
            hasta_status < 60,
            "status aparece en el byte {hasta_status}, demasiado tarde: {crudo}"
        );
        assert!(crudo.starts_with("{\"event\":"));
    }

    #[test]
    fn el_plan_declara_los_pasos_que_quiza_no_corran() {
        let mut def = DefinicionSecuencia::default();
        def.pasos_main.push(DefinicionPaso::nuevo("uno", 1));
        def.pasos_main.push(DefinicionPaso::nuevo("dos", 1));
        let escritor = EscritorQueFalla {
            lineas: Vec::new(),
            rechaza: usize::MAX,
            n: 0,
        };
        let mut sink = SinkEventos::nuevo(escritor, "run");
        sink.on_inicio_secuencia(&def);
        let v: serde_json::Value = serde_json::from_str(&sink.salida.lineas[0]).unwrap();
        let plan = v["plan"].as_array().unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1]["name"], "dos");
        assert_eq!(plan[1]["index"], 1);
        assert_eq!(v["events_version"], EVENTS_VERSION);
    }

    #[test]
    fn has_children_no_miente_sobre_una_llamada_que_no_entro() {
        let escritor = EscritorQueFalla {
            lineas: Vec::new(),
            rechaza: usize::MAX,
            n: 0,
        };
        let mut sink = SinkEventos::nuevo(escritor, "run");
        // Un `sequence_call` que se saltó: es una llamada, y no tiene hijos
        // que hayan viajado. Decir `true` por ser una llamada afirmaría que
        // hay hijos en el flujo que nadie va a encontrar.
        let r = ResultadoStep::nuevo("llamada", "skipped", "disable");
        sink.on_resultado(&r, &identidad("ccc", None, &[(Fase::Main, 0)]));
        let v: serde_json::Value = serde_json::from_str(&sink.salida.lineas[0]).unwrap();
        assert_eq!(v["has_children"], false);
        assert_eq!(v["final"], true);
    }
}
