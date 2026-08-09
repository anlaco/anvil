//! El lado consumidor del reporte: un `ResultSink` desacoplado con
//! lifecycle, a imagen del `ResultListener` de OpenTAP. El motor publica
//! eventos durante la ejecución; los sinks los consumen y se reparten el
//! trabajo de verter el resultado (consola, JSON, CSV, …) sin que el motor
//! sepa a quién reporta.
//!
//! El trait vive aquí (en `modelo`, la librería de datos) y no en un crate
//! de sinks a propósito: así el motor acota `impl ResultSink` sin depender
//! de ningún sink concreto, y `modelo` no gana dependencias (los cuerpos
//! por defecto son vacíos). Las implementaciones de sinks (consola/JSON/
//! CSV) viven en el crate `result_sink`.
//!
//! ## Lifecycle
//!
//! ```text
//! on_inicio_secuencia(def)          # lo que se va a correr
//!   on_inicio_paso(paso)            # antes de invocar el paso
//!   on_resultado(resultado)         # el ResultadoStep que devolvió
//!   on_fin_paso(paso)               # cierra el paso
//! on_fin_secuencia(resultado)       # lo que quedó, con estado agregado
//! ```
//!
//! Todos los métodos tienen cuerpo vacío por defecto: cada sink implementa
//! solo lo que le importa. El motor **no** sabe a quién reporta.
//!
//! ## Render en `on_fin_secuencia` (MVP)
//!
//! Los sinks de formato (consola/JSON/CSV) renderizan en
//! `on_fin_secuencia` a partir del `&ResultadoSecuencia` agregado, porque la
//! cabecera congelada (`=== nombre: estado ===`) necesita el estado
//! agregado, que solo se conoce al final. Los hooks de streaming
//! (`on_inicio_paso`, `on_resultado`, `on_fin_paso`) se disparan igual y
//! quedan listos para sinks de log/UI en vivo futuros; los sinks de formato
//! los ignoran. Esta es una adaptación del lifecycle propuesto en
//! `docs/diseno/reportes.md` (doc marcado como "propuesta").
//!
//! ## Errores: best-effort, sin `Result`
//!
//! El trait no devuelve `Result`: un fallo de escritura de un sink **no**
//! rompe la ejecución de la secuencia (es best-effort, RF-23). Los sinks
//! tragan sus errores de IO y, si quieren, los loguean a stderr —nunca a
//! stdout, que es territorio del sink de consola.

use crate::{DefinicionPaso, DefinicionSecuencia, ResultadoSecuencia, ResultadoStep};

/// Un consumidor del resultado de la secuencia. Ver la doc del módulo.
pub trait ResultSink {
    /// Al empezar a correr la secuencia. Recibe la **definición** (lo
    /// planeado), no los resultados.
    fn on_inicio_secuencia(&mut self, _secuencia: &DefinicionSecuencia) {}

    /// Antes de invocar un paso. Útil para sinks de log/streaming.
    fn on_inicio_paso(&mut self, _paso: &DefinicionPaso) {}

    /// El resultado que devolvió un paso ya corrido.
    fn on_resultado(&mut self, _resultado: &ResultadoStep) {}

    /// Cierra un paso. En el MVP lleva la definición del paso que acaba
    /// de terminar; un sink futuro podría querer también el resultado
    /// aquí (lo ya recibió en `on_resultado`).
    fn on_fin_paso(&mut self, _paso: &DefinicionPaso) {}

    /// Al terminar la secuencia. Recibe el `ResultadoSecuencia` agregado
    /// (con `estado()` y todos los `pasos`): es lo que los sinks de
    /// formato renderizan en el MVP.
    fn on_fin_secuencia(&mut self, _secuencia: &ResultadoSecuencia) {}
}

/// Varios sinks a la vez: delega cada hook en todos los sinks que contiene.
/// El bin construye uno de estos con los sinks activos y se lo pasa al
/// motor como un único `&mut impl ResultSink`.
pub struct SinkCompuesto<'a> {
    sinks: Vec<&'a mut dyn ResultSink>,
}

impl<'a> SinkCompuesto<'a> {
    pub fn nuevo(sinks: Vec<&'a mut dyn ResultSink>) -> Self {
        SinkCompuesto { sinks }
    }
}

impl<'a> ResultSink for SinkCompuesto<'a> {
    fn on_inicio_secuencia(&mut self, secuencia: &DefinicionSecuencia) {
        for s in &mut self.sinks {
            s.on_inicio_secuencia(secuencia);
        }
    }

    fn on_inicio_paso(&mut self, paso: &DefinicionPaso) {
        for s in &mut self.sinks {
            s.on_inicio_paso(paso);
        }
    }

    fn on_resultado(&mut self, resultado: &ResultadoStep) {
        for s in &mut self.sinks {
            s.on_resultado(resultado);
        }
    }

    fn on_fin_paso(&mut self, paso: &DefinicionPaso) {
        for s in &mut self.sinks {
            s.on_fin_paso(paso);
        }
    }

    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        for s in &mut self.sinks {
            s.on_fin_secuencia(secuencia);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DefinicionPaso;

    /// Un sink que cuenta los hooks que recibe, para verificar que el
    /// motor (o un driver del lifecycle) dispara la secuencia esperada.
    #[derive(Default)]
    struct Contador {
        inicios_secuencia: u32,
        inicios_paso: u32,
        resultados: u32,
        fines_paso: u32,
        fines_secuencia: u32,
    }

    impl ResultSink for Contador {
        fn on_inicio_secuencia(&mut self, _: &DefinicionSecuencia) {
            self.inicios_secuencia += 1;
        }
        fn on_inicio_paso(&mut self, _: &DefinicionPaso) {
            self.inicios_paso += 1;
        }
        fn on_resultado(&mut self, _: &ResultadoStep) {
            self.resultados += 1;
        }
        fn on_fin_paso(&mut self, _: &DefinicionPaso) {
            self.fines_paso += 1;
        }
        fn on_fin_secuencia(&mut self, _: &ResultadoSecuencia) {
            self.fines_secuencia += 1;
        }
    }

    #[test]
    fn sink_por_defecto_ignora_todo() {
        // Un sink sin implementar nada no pánico y no hace nada.
        struct Vacio;
        impl ResultSink for Vacio {}
        let mut v = Vacio;
        let def = DefinicionSecuencia::default();
        v.on_inicio_secuencia(&def);
        v.on_fin_secuencia(&ResultadoSecuencia::default());
    }

    #[test]
    fn compuesto_delega_en_todos() {
        let mut a = Contador::default();
        let mut b = Contador::default();
        let mut c = SinkCompuesto::nuevo(vec![&mut a, &mut b]);

        let def = DefinicionSecuencia::default();
        c.on_inicio_secuencia(&def);
        let paso = DefinicionPaso::nuevo("x", 1);
        c.on_inicio_paso(&paso);
        c.on_fin_paso(&paso);
        c.on_fin_secuencia(&ResultadoSecuencia::default());

        assert_eq!(a.inicios_secuencia, 1);
        assert_eq!(a.inicios_paso, 1);
        assert_eq!(a.fines_paso, 1);
        assert_eq!(a.fines_secuencia, 1);
        assert_eq!(
            b.inicios_secuencia, 1,
            "el segundo sink también recibe todo"
        );
    }
}
