//! Ask an executor what steps it serves, and check the sequence against the
//! answer (ADR-0021, issue #45).
//!
//! This is the piece that closes the exception ADR-0020 left open. Since a
//! step takes named inputs and returns named outputs, a typo —`canall` for
//! `canal`, `result.outputs.tensionn`— could only surface at run time, with
//! the unit already on the bench. Anvil now asks each endpoint **once**, at
//! start-up, and checks every name against what it was told.
//!
//! Two properties hold the design together, and both are about not lying:
//!
//! - **Asking beats inspecting.** TestStand reads the VI's connector pane, and
//!   runs out of road the moment the artifact carries no metadata (a C DLL
//!   without type information: *"you will have to manually specify the
//!   prototype"*). Asking works the same for WASM, for Python, for a box in
//!   another room, and for whatever comes next.
//! - **An executor may decline, and it shows.** A third party need not
//!   implement `Describe`. Then those steps are reported as unchecked — never
//!   an error, which would shut the door on third parties, and never silence,
//!   which is the false green of ADR-0019.

use modelo::proto::{Catalog, ParameterSpec, StepSpec, ValueType};
use modelo::{
    DefinicionPaso, DefinicionSecuencia, EntradaPaso, Programa, TipoPaso, ValorDefinicion,
};
use std::collections::BTreeMap;

use crate::{nombre_visible, Motor};

/// What one executor answered when asked for its catalog.
#[derive(Debug, Clone, PartialEq)]
pub enum Descripcion {
    /// It described itself. The catalog may legitimately be empty — that is a
    /// statement ("I serve nothing"), not silence.
    Describe(Catalog),
    /// It did not, and why. An old executor, one that does not implement the
    /// RPC, a stream that broke: from the engine's point of view they are the
    /// same thing, and none of them is an error.
    NoDescribe(String),
}

impl Descripcion {
    /// The catalog, if there is one.
    pub fn catalogo(&self) -> Option<&Catalog> {
        match self {
            Descripcion::Describe(c) => Some(c),
            Descripcion::NoDescribe(_) => None,
        }
    }
}

/// The catalogs of every connected executor, keyed by the same endpoint name
/// the routing uses (`EMBEDIDO` for the embedded one).
pub type Catalogos = BTreeMap<String, Descripcion>;

/// Something the sequence says that its executor contradicts.
///
/// Every variant is decidable **without measuring anything**, which is what
/// makes it belong here and not in a run: it is the detection rule of ADR-0019
/// applied to the one place ADR-0020 had to leave out.
#[derive(Debug, Clone, PartialEq)]
pub enum Hallazgo {
    /// The executor describes itself and this step is not in its catalog.
    PasoDesconocido {
        paso: String,
        ejecutor: String,
        conocidos: Vec<String>,
    },
    /// The step does not take an input the sequence sends. Never a warning:
    /// the executor would drop it and **measure something else**.
    EntradaDesconocida {
        paso: String,
        ejecutor: String,
        entrada: String,
        conocidas: Vec<String>,
    },
    /// A required input the sequence does not send.
    EntradaObligatoria {
        paso: String,
        ejecutor: String,
        entrada: String,
    },
    /// A literal of the wrong type. Only literals: an expression's type is not
    /// known until the run, and guessing it would be inventing a finding.
    TipoDeEntrada {
        paso: String,
        ejecutor: String,
        entrada: String,
        esperado: ValueType,
        recibido: ValueType,
    },
    /// An `assign` reads `result.outputs.<name>` and the step does not return
    /// it. This is the exact hole ADR-0020 §3 declared and left open.
    SalidaDesconocida {
        paso: String,
        ejecutor: String,
        salida: String,
        conocidas: Vec<String>,
    },
}

impl std::fmt::Display for Hallazgo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Hallazgo::PasoDesconocido {
                paso,
                ejecutor,
                conocidos,
            } => write!(
                f,
                "step '{paso}': executor '{ejecutor}' does not serve it (it serves: {})",
                lista(conocidos)
            ),
            Hallazgo::EntradaDesconocida {
                paso,
                ejecutor,
                entrada,
                conocidas,
            } => write!(
                f,
                "step '{paso}' ({ejecutor}): it takes no input called '{entrada}' \
                 (it takes: {})",
                lista(conocidas)
            ),
            Hallazgo::EntradaObligatoria {
                paso,
                ejecutor,
                entrada,
            } => write!(
                f,
                "step '{paso}' ({ejecutor}): the input '{entrada}' is required and the \
                 sequence does not send it"
            ),
            Hallazgo::TipoDeEntrada {
                paso,
                ejecutor,
                entrada,
                esperado,
                recibido,
            } => write!(
                f,
                "step '{paso}' ({ejecutor}): the input '{entrada}' is {} and the sequence \
                 sends a {}",
                esperado.name(),
                recibido.name()
            ),
            Hallazgo::SalidaDesconocida {
                paso,
                ejecutor,
                salida,
                conocidas,
            } => write!(
                f,
                "step '{paso}' ({ejecutor}): 'assign' reads result.outputs.{salida} and the \
                 step does not return it (it returns: {})",
                lista(conocidas)
            ),
        }
    }
}

/// A step nobody could check, and why. Not an error and not a pass: it is the
/// third answer ADR-0019 demands whenever Anvil cannot judge.
#[derive(Debug, Clone, PartialEq)]
pub struct SinComprobar {
    pub paso: String,
    pub ejecutor: String,
    pub motivo: String,
}

/// What came out of checking a program against the catalogs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Informe {
    /// Steps checked against a catalog, with nothing to say about them.
    pub comprobados: usize,
    pub hallazgos: Vec<Hallazgo>,
    pub sin_comprobar: Vec<SinComprobar>,
}

impl Informe {
    /// Whether the sequence contradicts a catalog. Unchecked steps do **not**
    /// count: not knowing is not the same as knowing it is wrong, and treating
    /// it as such would close the door on third-party executors.
    pub fn hay_hallazgos(&self) -> bool {
        !self.hallazgos.is_empty()
    }

    /// One line per executor that declined, for the human summary.
    pub fn resumen_sin_comprobar(&self) -> Vec<String> {
        let mut por_ejecutor: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
        for s in &self.sin_comprobar {
            por_ejecutor
                .entry((s.ejecutor.as_str(), s.motivo.as_str()))
                .or_default()
                .push(s.paso.as_str());
        }
        por_ejecutor
            .into_iter()
            .map(|((ejecutor, motivo), pasos)| {
                format!(
                    "{} step(s) unchecked on '{ejecutor}': {motivo} ({})",
                    pasos.len(),
                    pasos.join(", ")
                )
            })
            .collect()
    }
}

fn lista(nombres: &[String]) -> String {
    if nombres.is_empty() {
        "none".to_string()
    } else {
        nombres.join(", ")
    }
}

impl Motor {
    /// Asks every connected executor for its catalog — **once per endpoint**,
    /// and this is the only place that asks.
    ///
    /// Not once per step, and the reason is not cost: asking step by step means
    /// finding out at step 47 that a name is wrong, with the unit half tested,
    /// and it lets the catalog change mid-run, which makes the report
    /// impossible to reconstruct (ADR-0021 §3).
    ///
    /// **Any failure is `NoDescribe`, never a run-stopping error.** An executor
    /// that does not implement the RPC answers `UNIMPLEMENTED`, which reaches
    /// wasi-grpc v0.1 as a stream that closed without a message — the same
    /// shape as several other harmless outcomes, and none of them is worth
    /// refusing to test over. If the connection really is broken, the first
    /// `Invoke` says so, loudly and in the right place.
    pub fn describe_ejecutores(&mut self) -> Catalogos {
        let endpoints: Vec<String> = self.conexiones.keys().cloned().collect();
        let mut fuera = BTreeMap::new();
        for endpoint in endpoints {
            fuera.insert(endpoint.clone(), self.describe_uno(&endpoint));
        }
        fuera
    }

    fn describe_uno(&mut self, endpoint: &str) -> Descripcion {
        use modelo::proto::{CatalogRequest, CONTRACT, ROUTE_DESCRIBE};
        use prost::Message;

        let cliente = match self.conexiones.get_mut(endpoint) {
            Some(c) => c,
            None => return Descripcion::NoDescribe("there is no open connection".into()),
        };
        let peticion = CatalogRequest { contract: CONTRACT };
        let bytes = match cliente.unaria(ROUTE_DESCRIBE, &peticion.encode_to_vec()) {
            Ok(b) => b,
            Err(e) => return Descripcion::NoDescribe(format!("it does not answer Describe ({e})")),
        };
        match Catalog::decode(&bytes[..]) {
            // `describes == false` is the proto3 default, so an empty body, an
            // old peer and an executor that declines all land here — and all of
            // them mean "do not check me", never "I serve no steps".
            Ok(c) if !c.describes => {
                Descripcion::NoDescribe("it does not describe its catalog".into())
            }
            Ok(c) => Descripcion::Describe(c),
            Err(e) => Descripcion::NoDescribe(format!("its catalog is unreadable ({e})")),
        }
    }
}

/// Checks a whole program —root, external subsequences and inline ones—
/// against the catalogs of the executors that will serve it.
///
/// Pure: it neither connects nor runs. The connecting is
/// [`Motor::describe_ejecutores`]'s job, which is what lets this be tested
/// against catalogs written by hand, and what lets `--validate` reuse it.
pub fn comprueba_programa(programa: &Programa, catalogos: &Catalogos) -> Informe {
    let mut informe = Informe::default();
    comprueba_secuencia(&programa.raiz, programa, catalogos, &mut informe);
    for sec in programa.archivos.values() {
        comprueba_secuencia(sec, programa, catalogos, &mut informe);
    }
    informe
}

fn comprueba_secuencia(
    secuencia: &DefinicionSecuencia,
    programa: &Programa,
    catalogos: &Catalogos,
    informe: &mut Informe,
) {
    for def in secuencia
        .pasos_setup
        .iter()
        .chain(&secuencia.pasos_main)
        .chain(&secuencia.pasos_cleanup)
    {
        comprueba_paso(def, programa, catalogos, informe);
    }
    for sub in secuencia.subsecuencias.values() {
        comprueba_secuencia(sub, programa, catalogos, informe);
    }
}

fn comprueba_paso(
    def: &DefinicionPaso,
    programa: &Programa,
    catalogos: &Catalogos,
    informe: &mut Informe,
) {
    // Only steps that cross the wire. A `statement`, a `pass_fail` or a
    // `sequence_call` is the engine's own business and no executor describes
    // it (ADR-0009, ADR-0010, ADR-0018).
    if def.tipo != TipoPaso::Grpc {
        return;
    }
    // A disabled step is registered as `skipped` **without asking anyone**
    // (RF-34): it never reaches an executor, so it cannot measure the wrong
    // thing, and refusing to run a whole sequence over a step its author
    // explicitly turned off would be the check overreaching. This check
    // mirrors what actually crosses the wire, and nothing else.
    if def.disable {
        return;
    }
    let endpoint = match Motor::resolver_endpoint(def, programa) {
        Ok(e) => e.to_string(),
        // An undeclared executor is already a load error, and a `wasm` one
        // reaching here means the engine is running without its host. Neither
        // is a signature finding: reporting it as one would name the wrong
        // problem.
        Err(_) => return,
    };
    let visible = nombre_visible(&endpoint).to_string();
    let catalogo = match catalogos.get(&endpoint) {
        Some(Descripcion::Describe(c)) => c,
        Some(Descripcion::NoDescribe(motivo)) => {
            informe.sin_comprobar.push(SinComprobar {
                paso: def.nombre.clone(),
                ejecutor: visible,
                motivo: motivo.clone(),
            });
            return;
        }
        None => {
            informe.sin_comprobar.push(SinComprobar {
                paso: def.nombre.clone(),
                ejecutor: visible,
                motivo: "it was not asked".into(),
            });
            return;
        }
    };

    let spec = match catalogo.step(&def.nombre) {
        Some(s) => s,
        None => {
            informe.hallazgos.push(Hallazgo::PasoDesconocido {
                paso: def.nombre.clone(),
                ejecutor: visible,
                conocidos: catalogo.steps.iter().map(|s| s.name.clone()).collect(),
            });
            return;
        }
    };

    comprueba_entradas(def, spec, &visible, informe);
    comprueba_salidas(def, spec, &visible, informe);
    informe.comprobados += 1;
}

fn comprueba_entradas(
    def: &DefinicionPaso,
    spec: &StepSpec,
    ejecutor: &str,
    informe: &mut Informe,
) {
    let entradas = def.entradas.as_deref().unwrap_or(&[]);
    for (nombre, valor) in entradas {
        let Some(p) = spec.input(nombre) else {
            informe.hallazgos.push(Hallazgo::EntradaDesconocida {
                paso: def.nombre.clone(),
                ejecutor: ejecutor.to_string(),
                entrada: nombre.clone(),
                conocidas: spec.inputs.iter().map(|p| p.name.clone()).collect(),
            });
            continue;
        };
        if let Some(h) = comprueba_tipo(def, p, valor, ejecutor) {
            informe.hallazgos.push(h);
        }
    }
    for p in &spec.inputs {
        if p.required && !entradas.iter().any(|(n, _)| *n == p.name) {
            informe.hallazgos.push(Hallazgo::EntradaObligatoria {
                paso: def.nombre.clone(),
                ejecutor: ejecutor.to_string(),
                entrada: p.name.clone(),
            });
        }
    }
}

/// The type of one input against its spec — **only when it is a literal**.
///
/// An expression's type is not known until the run: `${locals.n}` may hold a
/// number or a text, and calling it a finding without knowing would be
/// inventing one. That parameter is simply not type-checked; its *name* still
/// is, which is what the typo costs today.
fn comprueba_tipo(
    def: &DefinicionPaso,
    p: &ParameterSpec,
    valor: &EntradaPaso,
    ejecutor: &str,
) -> Option<Hallazgo> {
    let esperado = p.value_type();
    if esperado == ValueType::Unspecified {
        return None;
    }
    let recibido = match valor {
        EntradaPaso::Literal(ValorDefinicion::Numero(_)) => ValueType::Number,
        EntradaPaso::Literal(ValorDefinicion::Texto(_)) => ValueType::Text,
        EntradaPaso::Literal(ValorDefinicion::Bool(_)) => ValueType::Boolean,
        EntradaPaso::Expresion(_) => return None,
    };
    (recibido != esperado).then(|| Hallazgo::TipoDeEntrada {
        paso: def.nombre.clone(),
        ejecutor: ejecutor.to_string(),
        entrada: p.name.clone(),
        esperado,
        recibido,
    })
}

fn comprueba_salidas(def: &DefinicionPaso, spec: &StepSpec, ejecutor: &str, informe: &mut Informe) {
    let mut leidas = Vec::new();
    for a in def.asigna.as_deref().unwrap_or(&[]) {
        salidas_leidas(&a.expr, &mut leidas);
    }
    for salida in leidas {
        if !spec.outputs.iter().any(|o| o.name == salida) {
            informe.hallazgos.push(Hallazgo::SalidaDesconocida {
                paso: def.nombre.clone(),
                ejecutor: ejecutor.to_string(),
                salida,
                conocidas: spec.outputs.iter().map(|o| o.name.clone()).collect(),
            });
        }
    }
}

/// Every `result.outputs.<name>` an expression reads, walking the whole tree:
/// the read can be buried inside an operation (`result.outputs.t * 2`), not
/// just standing alone.
fn salidas_leidas(e: &expr::Expresion, fuera: &mut Vec<String>) {
    use expr::{Expresion, Scope};
    match e {
        Expresion::Var { scope, campo } => {
            if *scope == Scope::Resultado {
                if let Some(resto) = campo.strip_prefix(expr::CAMPO_SALIDAS) {
                    if let Some(nombre) = resto.strip_prefix('.') {
                        if !nombre.is_empty() && !fuera.iter().any(|n| n == nombre) {
                            fuera.push(nombre.to_string());
                        }
                    }
                }
            }
        }
        Expresion::BinOp { izq, der, .. } => {
            salidas_leidas(izq, fuera);
            salidas_leidas(der, fuera);
        }
        Expresion::UnOp { operando, .. } => salidas_leidas(operando, fuera),
        Expresion::Lit(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelo::proto::{OutputSpec, ParameterSpec};
    use modelo::{Asignacion, DefinicionPaso, DefinicionSecuencia};
    use std::collections::HashMap;

    /// The catalog of an executor that serves `medir_voltaje(canal?, offset?)`
    /// and returns `temperatura` — the same shape the embedded executor
    /// publishes, written by hand so these tests need no network.
    fn catalogo_demo() -> Catalog {
        Catalog::descrito(vec![StepSpec {
            name: "medir_voltaje".into(),
            inputs: vec![
                ParameterSpec::optional("canal", ValueType::Number, expr::Value::Numero(1.0)),
                ParameterSpec::required("etiqueta", ValueType::Text),
            ],
            outputs: vec![OutputSpec::nueva("temperatura", ValueType::Number, "")],
            doc: String::new(),
        }])
    }

    fn catalogos_del_embebido(c: Descripcion) -> Catalogos {
        let mut m = BTreeMap::new();
        m.insert(crate::EMBEDIDO.to_string(), c);
        m
    }

    /// An otherwise empty sequence: only its steps matter here.
    fn secuencia(nombre: &str) -> DefinicionSecuencia {
        DefinicionSecuencia {
            nombre: nombre.to_string(),
            pasos_setup: Vec::new(),
            pasos_main: Vec::new(),
            pasos_cleanup: Vec::new(),
            locals: HashMap::new(),
            parameters: HashMap::new(),
            file_globals: HashMap::new(),
            subsecuencias: HashMap::new(),
        }
    }

    fn programa_con(pasos: Vec<DefinicionPaso>) -> Programa {
        let mut raiz = secuencia("s");
        raiz.pasos_main = pasos;
        Programa {
            raiz,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        }
    }

    fn paso(nombre: &str) -> DefinicionPaso {
        let mut d = DefinicionPaso::nuevo(nombre, 1);
        d.entradas = Some(vec![(
            "etiqueta".to_string(),
            EntradaPaso::Literal(ValorDefinicion::Texto("banco-3".into())),
        )]);
        d
    }

    /// The typo of issue #45: `canall` for `canal`. Without a catalog it could
    /// only surface at run time, with the unit already on the bench.
    ///
    /// Seen failing by writing `canal` (the right name): the finding
    /// disappears, which is what says the check is looking at the name and not
    /// merely counting parameters.
    #[test]
    fn una_entrada_que_el_paso_no_toma_es_un_hallazgo() {
        let mut d = paso("medir_voltaje");
        d.entradas.as_mut().unwrap().push((
            "canall".to_string(),
            EntradaPaso::Literal(ValorDefinicion::Numero(3.0)),
        ));
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert_eq!(informe.hallazgos.len(), 1, "{:?}", informe.hallazgos);
        assert!(matches!(
            &informe.hallazgos[0],
            Hallazgo::EntradaDesconocida { entrada, .. } if entrada == "canall"
        ));
        // Y el mensaje trae los nombres buenos: el error es casi siempre un
        // dedazo, y la respuesta cabe en la misma línea.
        assert!(informe.hallazgos[0].to_string().contains("canal, etiqueta"));
    }

    /// The hole ADR-0020 §3 declared and left open: `assign` reads
    /// `result.outputs.<name>` and the loader cannot check it — until now.
    #[test]
    fn una_salida_que_el_paso_no_devuelve_es_un_hallazgo() {
        let mut d = paso("medir_voltaje");
        d.asigna = Some(vec![Asignacion {
            var: "t".to_string(),
            // Buried inside an operation on purpose: reading only the bare
            // form would miss `result.outputs.t * 2`.
            expr: expr::parse_expresion("result.outputs.temperaturaa * 2").unwrap(),
        }]);
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(matches!(
            &informe.hallazgos[..],
            [Hallazgo::SalidaDesconocida { salida, .. }] if salida == "temperaturaa"
        ));
    }

    #[test]
    fn una_entrada_obligatoria_que_falta_es_un_hallazgo() {
        let mut d = paso("medir_voltaje");
        d.entradas = None;
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(matches!(
            &informe.hallazgos[..],
            [Hallazgo::EntradaObligatoria { entrada, .. }] if entrada == "etiqueta"
        ));
    }

    /// An optional input that is missing is **not** a finding: that is the
    /// step's business, and reporting it would make every sequence noisy.
    #[test]
    fn una_entrada_opcional_que_falta_no_es_nada() {
        let informe = comprueba_programa(
            &programa_con(vec![paso("medir_voltaje")]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(!informe.hay_hallazgos(), "{:?}", informe.hallazgos);
        assert_eq!(informe.comprobados, 1);
    }

    #[test]
    fn un_literal_del_tipo_equivocado_es_un_hallazgo() {
        let mut d = paso("medir_voltaje");
        d.entradas = Some(vec![(
            "etiqueta".to_string(),
            EntradaPaso::Literal(ValorDefinicion::Numero(3.0)),
        )]);
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(matches!(
            &informe.hallazgos[..],
            [Hallazgo::TipoDeEntrada {
                esperado: ValueType::Text,
                recibido: ValueType::Number,
                ..
            }]
        ));
    }

    /// An expression's type is not known until the run. Calling it a finding
    /// would be inventing one — and the sequence would be rejected for a
    /// `${locals.x}` that is perfectly fine.
    ///
    /// The expression is bound to a **number** input on purpose: any guess the
    /// code might make would have to be one of the three types, and the only
    /// way this test can see a guess happen is if the declared type is not the
    /// one guessed. Seen failing by making an expression count as text.
    #[test]
    fn el_tipo_de_una_expresion_no_se_adivina() {
        let mut d = paso("medir_voltaje");
        d.entradas.as_mut().unwrap().push((
            "canal".to_string(),
            EntradaPaso::Expresion(expr::parse_expresion("locals.x").unwrap()),
        ));
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(!informe.hay_hallazgos(), "{:?}", informe.hallazgos);
    }

    /// ADR-0021 §4: an executor may decline to describe itself. Then the step
    /// is **unchecked** — not a finding (that would shut the door on third
    /// parties) and not silence (that is the false green of ADR-0019).
    #[test]
    fn un_ejecutor_que_no_describe_deja_los_pasos_sin_comprobar() {
        let informe = comprueba_programa(
            &programa_con(vec![paso("medir_voltaje"), paso("verificar_led")]),
            &catalogos_del_embebido(Descripcion::NoDescribe("no contesta Describe".into())),
        );
        assert!(!informe.hay_hallazgos(), "no describir no es contradecir");
        assert_eq!(informe.comprobados, 0);
        assert_eq!(informe.sin_comprobar.len(), 2);
        let resumen = informe.resumen_sin_comprobar();
        assert_eq!(resumen.len(), 1, "una línea por ejecutor y motivo");
        assert!(
            resumen[0].contains("2 step(s) unchecked") && resumen[0].contains("embebido"),
            "el aviso cuenta y nombra: {}",
            resumen[0]
        );
    }

    /// The difference `Catalog.describes` exists for. An executor that
    /// positively serves nothing contradicts every step routed to it; one that
    /// stays silent contradicts none. Reading silence as an empty catalog
    /// would turn a perfectly good sequence into a wall of findings.
    #[test]
    fn un_catalogo_vacio_no_es_lo_mismo_que_no_describir() {
        let pasos = vec![paso("medir_voltaje")];
        let vacio = comprueba_programa(
            &programa_con(pasos.clone()),
            &catalogos_del_embebido(Descripcion::Describe(Catalog::descrito(Vec::new()))),
        );
        assert!(matches!(
            &vacio.hallazgos[..],
            [Hallazgo::PasoDesconocido { .. }]
        ));

        let mudo = comprueba_programa(
            &programa_con(pasos),
            &catalogos_del_embebido(Descripcion::NoDescribe("no describe".into())),
        );
        assert!(!mudo.hay_hallazgos());
    }

    /// A `statement`, a `pass_fail` or a `sequence_call` never crosses the
    /// wire, so no executor describes it. Checking them against a catalog
    /// would reject every sequence that uses them.
    #[test]
    fn los_pasos_que_no_cruzan_el_cable_no_se_comprueban() {
        let mut d = DefinicionPaso::nuevo("no_esta_en_el_catalogo", 1);
        d.tipo = TipoPaso::Statement;
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(!informe.hay_hallazgos());
        assert!(
            informe.sin_comprobar.is_empty(),
            "ni siquiera sin comprobar"
        );
    }

    /// The check reaches the whole program, not just `main`: setup, cleanup
    /// and inline subsequences included. A typo in `cleanup` is exactly the
    /// one nobody notices.
    #[test]
    fn se_comprueba_el_programa_entero_y_no_solo_main() {
        let mut sub = secuencia("sub");
        sub.pasos_main = vec![paso("fantasma")];
        let mut raiz = secuencia("s");
        raiz.pasos_setup = vec![paso("medir_voltaje")];
        raiz.pasos_main = vec![paso("medir_voltaje")];
        raiz.pasos_cleanup = vec![paso("fantasma")];
        raiz.subsecuencias.insert("sub".to_string(), sub);
        let programa = Programa {
            raiz,
            archivos: HashMap::new(),
            ejecutores: HashMap::new(),
        };
        let informe = comprueba_programa(
            &programa,
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert_eq!(informe.comprobados, 2, "setup y main");
        assert_eq!(
            informe.hallazgos.len(),
            2,
            "cleanup y la subsecuencia inline: {:?}",
            informe.hallazgos
        );
    }

    /// A `disable: true` step never reaches an executor (RF-34), so it cannot
    /// measure the wrong thing. Flagging it would refuse to run a sequence
    /// over a step its author explicitly turned off — the check overreaching.
    ///
    /// Seen failing by removing the `disable` guard: `ejemplos/variables.yaml`,
    /// which ships a disabled `paso_obsoleto`, stops running.
    #[test]
    fn un_paso_deshabilitado_no_se_comprueba() {
        let mut d = paso("fantasma");
        d.disable = true;
        let informe = comprueba_programa(
            &programa_con(vec![d]),
            &catalogos_del_embebido(Descripcion::Describe(catalogo_demo())),
        );
        assert!(!informe.hay_hallazgos(), "{:?}", informe.hallazgos);
        assert_eq!(informe.comprobados, 0);
        assert!(informe.sin_comprobar.is_empty());
    }
}
