//! Los pasos de la secuencia de ejemplo "basica", con comportamiento
//! simulado — forma parte de la especificación del producto.
//!
//! Todos toman `intento` aunque la mayoría lo ignore: es la convención
//! uniforme que necesita el despacho por nombre.

use modelo::ResultadoStep;

/// Falla el primer intento y pasa a partir del segundo: simula un equipo
/// que tarda en responder, y ejercita los reintentos del motor.
pub fn conectar(intento: i32) -> ResultadoStep {
    if intento < 2 {
        ResultadoStep::nuevo(
            "conectar_equipo",
            "fallo",
            format!("el equipo no respondió (intento {intento})"),
        )
    } else {
        ResultadoStep::nuevo(
            "conectar_equipo",
            "paso",
            format!("equipo conectado (intento {intento})"),
        )
    }
}

/// Mide 4.2 V y devuelve la medida **sin conocer el umbral**: en M3 los
/// límites son datos first-class (RF-29), viven en la secuencia (YAML) y los
/// evalúa el motor (ADR-0008). El paso solo mide y reporta que la medición
/// fue bien (`paso`); el motor compara 4.2 contra el rango declarado y, si no
/// cumple, convierte el estado en `fallo`.
///
/// Por eso mide 4.2 contra un rango 4.5–5.5 declarado en `ejemplos/basica.yaml`
/// y el resultado final de la secuencia sigue siendo `fallo` — solo que ahora
/// el umbral ya no está grabado aquí.
pub fn medir_voltaje(_intento: i32) -> ResultadoStep {
    let valor = 4.2;
    ResultadoStep::medido_valor("medir_voltaje", "paso", "medido: 4.2 V", valor)
}

/// Pass/fail (RF-25): hace algo y reporta `paso`/`fallo` **sin medida**. El
/// caso más simple de step type built-in.
pub fn verificar_led(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("verificar_led", "paso", "led encendido")
}

/// Action (RF-27): ejecuta una acción (abrir un relé) y su estado es `paso`
/// si no hubo `error` — sin criterio de aceptación, solo éxito técnico. Aquí
/// siempre pasa; en hardware real devolvería `error` si el relé no responde.
pub fn abrir_rele(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("abrir_rele", "paso", "relé abierto")
}

pub fn desconectar(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("desconectar_equipo", "paso", "equipo desconectado")
}

/// Plug-in del process model Sequential (M5, RF-38): identifica el UUT.
/// En fábrica leería el número de serie (escáner, puerto serie…); aquí es
/// simulado: devuelve `paso` con el serial en `mensaje`, que el `asigna`
/// del PM vuelca a `locals.uut_id`. Es un paso `grpc` despachado por el
/// ejecutor, no un callback motor-side — así el PM es datos (ADR-0005).
pub fn identificar_uut(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("identificar_uut", "paso", "UUT-DEMO-001")
}

/// Plug-in del process model Sequential (M5, RF-38): notifica el
/// resultado del UUT (piloto, buzzer, MES…). Simulado: siempre `paso`.
/// En el PM canónico corre en `cleanup`, tras el `sequence_call` al
/// usuario, así el `asigna` del call ya dejó en `locals.estado_usuario`
/// el agregado de la secuencia del usuario.
pub fn notificar_resultado(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("notificar_resultado", "paso", "UUT notificado")
}

/// Despacho por nombre: el motor invoca los pasos por su nombre, nunca con
/// una llamada directa, así que este es el único punto donde el nombre del
/// cable se ata a una función. Un nombre desconocido es `error`, no pánico:
/// una secuencia mal escrita no debe tumbar el ejecutor.
pub fn despacha(nombre: &str, intento: i32) -> ResultadoStep {
    match nombre {
        "conectar_equipo" => conectar(intento),
        "medir_voltaje" => medir_voltaje(intento),
        "verificar_led" => verificar_led(intento),
        "abrir_rele" => abrir_rele(intento),
        "desconectar_equipo" => desconectar(intento),
        "identificar_uut" => identificar_uut(intento),
        "notificar_resultado" => notificar_resultado(intento),
        _ => ResultadoStep::nuevo("desconocido", "error", "paso no reconocido"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conectar_falla_el_primero_y_pasa_el_segundo() {
        assert_eq!(conectar(1).estado, "fallo");
        assert_eq!(conectar(2).estado, "paso");
        assert_eq!(conectar(2).mensaje, "equipo conectado (intento 2)");
    }

    #[test]
    fn voltaje_mide_y_pasa_el_paso_no_conoce_el_umbral() {
        // En M3 el paso mide y devuelve `paso` (medición OK); el motor
        // evalúa el límite del YAML. El paso no trae límites embebidos.
        let r = medir_voltaje(1);
        assert_eq!(r.estado, "paso", "el paso solo mide: la regla la aplica el motor");
        assert_eq!(r.valor_medido, Some(4.2));
        assert_eq!(r.limite_min, None, "el umbral vive en el YAML, no aquí");
        assert_eq!(r.limite_max, None);
    }

    #[test]
    fn action_abrir_rele_pasa_sin_medida() {
        let r = abrir_rele(1);
        assert_eq!(r.estado, "paso");
        assert_eq!(r.valor_medido, None, "un action no mide");
    }

    #[test]
    fn nombre_desconocido_es_error() {
        let r = despacha("no_existe", 1);
        assert_eq!(r.estado, "error");
        assert_eq!(r.nombre, "desconocido");
    }

    #[test]
    fn despacho_por_nombre() {
        assert_eq!(despacha("verificar_led", 1).nombre, "verificar_led");
        assert_eq!(despacha("abrir_rele", 1).estado, "paso");
        assert_eq!(despacha("desconectar_equipo", 1).estado, "paso");
    }

    #[test]
    fn identificar_uut_devuelve_serial_demo() {
        let r = identificar_uut(1);
        assert_eq!(r.estado, "paso");
        assert_eq!(r.mensaje, "UUT-DEMO-001");
        assert_eq!(r.nombre, "identificar_uut");
    }

    #[test]
    fn notificar_resultado_pasa() {
        let r = notificar_resultado(1);
        assert_eq!(r.estado, "paso");
        assert_eq!(r.nombre, "notificar_resultado");
    }

    #[test]
    fn despacha_resuelve_los_dos_plugines_del_pm() {
        assert_eq!(despacha("identificar_uut", 1).nombre, "identificar_uut");
        assert_eq!(despacha("notificar_resultado", 1).estado, "paso");
    }
}
