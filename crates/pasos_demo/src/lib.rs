//! Los pasos de la secuencia de ejemplo "basica". Port de
//! `secuenciador/pasos_demo.ana`, con el mismo comportamiento simulado —
//! forma parte de la especificación que la migración conserva.
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

/// Mide 4.2 V contra un rango de 4.5–5.5: falla siempre, a propósito. Es
/// lo que hace que la secuencia de ejemplo corte el Main y demuestre la
/// semántica de ejecución.
pub fn medir_voltaje(_intento: i32) -> ResultadoStep {
    let valor = 4.2;
    let (min, max) = (4.5, 5.5);
    if valor < min {
        ResultadoStep::medido(
            "medir_voltaje",
            "fallo",
            "voltaje fuera de rango",
            valor,
            min,
            max,
        )
    } else {
        ResultadoStep::medido(
            "medir_voltaje",
            "paso",
            "voltaje dentro de rango",
            valor,
            min,
            max,
        )
    }
}

pub fn verificar_led(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("verificar_led", "paso", "led encendido")
}

pub fn desconectar(_intento: i32) -> ResultadoStep {
    ResultadoStep::nuevo("desconectar_equipo", "paso", "equipo desconectado")
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
        "desconectar_equipo" => desconectar(intento),
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
    fn voltaje_fuera_de_rango() {
        let r = medir_voltaje(1);
        assert_eq!(r.estado, "fallo");
        assert_eq!(r.valor_medido, Some(4.2));
        assert_eq!(r.limite_min, Some(4.5));
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
        assert_eq!(despacha("desconectar_equipo", 1).estado, "paso");
    }
}
