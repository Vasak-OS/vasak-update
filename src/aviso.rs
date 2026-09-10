//! Cuándo avisar, y cuándo callarse.
//!
//! Es la parte que decide si el aviso sirve para algo. Avisar todos los días
//! de los mismos tres paquetes es la forma más rápida de que la gente aprenda
//! a ignorar el aviso, y a partir de ahí el día que importe tampoco lo va a
//! leer.
//!
//! Las reglas, en orden y con lo que cada una evita:
//!
//! 1. **Sin actualizaciones, nada.** Obvio, y va primero porque es el caso
//!    normal: la mayoría de las corridas no avisan.
//! 2. **Si es exactamente lo mismo que la última vez, nada** — salvo que haya
//!    pasado mucho. Es lo que evita el aviso diario idéntico. `arch-update`
//!    usa la misma regla y por eso su aviso no cansa.
//! 3. **Si cambia el kernel, se avisa igual.** Aunque el conjunto sea el
//!    mismo: es lo único de la lista que pide reiniciar, y postergarlo tiene
//!    consecuencias visibles —módulos que no cargan, una impresora que deja de
//!    aparecer—.
//! 4. **Y si hace mucho que no se avisa, se avisa igual.** Un sistema rolling
//!    sin actualizar durante meses es la forma más segura de romperlo, y el
//!    silencio por «ya te dije» no puede durar para siempre.

/// Cada cuántos días se vuelve a insistir con lo mismo.
///
/// Una semana. Menos molesta; más deja de ser un recordatorio.
const DIAS_PARA_INSISTIR: u64 = 7;

const SEGUNDOS_POR_DIA: u64 = 86_400;

/// Lo que se recuerda de la última vez que se avisó.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recuerdo {
    /// La huella del conjunto que se avisó. Vacío si nunca se avisó.
    pub huella: String,
    /// Cuándo, en segundos desde la época. Cero si nunca.
    pub cuando: u64,
}

/// Por qué se avisa, o por qué no.
///
/// Se devuelve el motivo y no un `bool` porque el motivo es lo que se escribe
/// en el registro cuando alguien pregunta por qué el aviso no apareció, que es
/// la pregunta que se hace cuando esto falla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// No hay nada que actualizar.
    NadaQueAvisar,
    /// Es lo mismo que ya se avisó y hace poco.
    YaAvisado,
    /// Hay algo nuevo respecto de la última vez.
    HayNovedades,
    /// Cambia el kernel, así que se avisa aunque sea lo mismo.
    CambiaElKernel,
    /// Pasó demasiado desde el último aviso.
    HaceMucho,
}

impl Decision {
    pub fn avisa(self) -> bool {
        !matches!(self, Decision::NadaQueAvisar | Decision::YaAvisado)
    }
}

/// La huella de un conjunto de actualizaciones.
///
/// Tiene que cambiar cuando cambia **qué** se actualiza o **a qué versión**,
/// y no cuando cambia el orden. Por eso las líneas se ordenan antes: `pacman`
/// las devuelve ordenadas hoy, y apoyarse en eso haría que un cambio suyo se
/// tradujera en un aviso repetido todos los días.
///
/// No es una huella criptográfica y no tiene por qué serlo: acá nadie ataca,
/// sólo se compara con la anterior. Es la de FNV-1a, que entra en diez líneas
/// y no agrega una dependencia.
pub fn huella(entradas: &[String]) -> String {
    let mut ordenadas: Vec<&str> = entradas.iter().map(String::as_str).collect();
    ordenadas.sort_unstable();

    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for entrada in ordenadas {
        for byte in entrada.as_bytes() {
            h ^= u64::from(*byte);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        // El separador importa: sin él, `["ab", "c"]` y `["a", "bc"]` darían
        // la misma huella y un cambio real pasaría por «ya avisado».
        h ^= 0xff;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// Si corresponde avisar.
pub fn decidir(
    cuantas: usize,
    cambia_el_kernel: bool,
    huella_actual: &str,
    recuerdo: &Recuerdo,
    ahora: u64,
) -> Decision {
    if cuantas == 0 {
        return Decision::NadaQueAvisar;
    }
    if huella_actual != recuerdo.huella {
        return Decision::HayNovedades;
    }
    if cambia_el_kernel {
        return Decision::CambiaElKernel;
    }
    // `saturating_sub` y no una resta: si el reloj se atrasó —arrancar sin
    // batería en la placa, o volver de una suspensión larga— una resta con
    // desbordamiento daría un número enorme y avisaría siempre.
    if ahora.saturating_sub(recuerdo.cuando) >= DIAS_PARA_INSISTIR * SEGUNDOS_POR_DIA {
        return Decision::HaceMucho;
    }
    Decision::YaAvisado
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recuerdo(huella: &str, hace_dias: u64, ahora: u64) -> Recuerdo {
        Recuerdo {
            huella: huella.into(),
            cuando: ahora - hace_dias * SEGUNDOS_POR_DIA,
        }
    }

    const AHORA: u64 = 1_800_000_000;

    /// **Lo mismo de ayer no se vuelve a avisar.**
    ///
    /// Es la regla que hace que el aviso siga sirviendo: uno diario e idéntico
    /// enseña a ignorarlo, y a partir de ahí el que importa tampoco se lee.
    #[test]
    fn lo_mismo_de_ayer_no_se_repite() {
        let h = huella(&["pacman 7.0.0-8 -> 7.0.0-9".into()]);
        let r = recuerdo(&h, 1, AHORA);
        assert_eq!(decidir(1, false, &h, &r, AHORA), Decision::YaAvisado);
        assert!(!decidir(1, false, &h, &r, AHORA).avisa());
    }

    /// **Pero un paquete más ya es otra cosa.**
    #[test]
    fn un_paquete_nuevo_vuelve_a_avisar() {
        let ayer = huella(&["pacman 7.0.0-8 -> 7.0.0-9".into()]);
        let hoy = huella(&["pacman 7.0.0-8 -> 7.0.0-9".into(), "firefox 1 -> 2".into()]);
        let r = recuerdo(&ayer, 1, AHORA);
        assert_eq!(decidir(2, false, &hoy, &r, AHORA), Decision::HayNovedades);
    }

    /// **El kernel avisa siempre, aunque sea lo mismo.**
    ///
    /// Es lo único de la lista que pide reiniciar, y postergarlo se nota:
    /// módulos que no cargan, una impresora que deja de aparecer.
    #[test]
    fn el_kernel_insiste() {
        let h = huella(&["linux 7.2.3-1 -> 7.2.4-1".into()]);
        let r = recuerdo(&h, 1, AHORA);
        assert_eq!(decidir(1, true, &h, &r, AHORA), Decision::CambiaElKernel);
        assert!(decidir(1, true, &h, &r, AHORA).avisa());
    }

    /// **Y el silencio no dura para siempre.**
    #[test]
    fn despues_de_una_semana_se_vuelve_a_insistir() {
        let h = huella(&["pacman 1 -> 2".into()]);
        assert_eq!(
            decidir(1, false, &h, &recuerdo(&h, 6, AHORA), AHORA),
            Decision::YaAvisado
        );
        assert_eq!(
            decidir(1, false, &h, &recuerdo(&h, 7, AHORA), AHORA),
            Decision::HaceMucho
        );
    }

    /// **Sin actualizaciones no se avisa, pase lo que pase.**
    #[test]
    fn sin_actualizaciones_no_se_avisa() {
        let r = Recuerdo::default();
        assert_eq!(
            decidir(0, true, "loquesea", &r, AHORA),
            Decision::NadaQueAvisar
        );
        assert!(!decidir(0, true, "loquesea", &r, AHORA).avisa());
    }

    /// **La primera vez se avisa.**
    ///
    /// Sin recuerdo, la huella no coincide y hay novedades. Sale del caso
    /// general y no de una rama aparte, que es lo que se olvida de probar.
    #[test]
    fn la_primera_vez_se_avisa() {
        let h = huella(&["pacman 1 -> 2".into()]);
        assert_eq!(
            decidir(1, false, &h, &Recuerdo::default(), AHORA),
            Decision::HayNovedades
        );
    }

    /// **La huella no depende del orden, y sí de dónde termina cada entrada.**
    ///
    /// Lo primero, porque apoyarse en el orden de `pacman` haría que un cambio
    /// suyo se viera como un conjunto nuevo todos los días. Lo segundo es el
    /// error clásico de concatenar sin separador: `["ab","c"]` y `["a","bc"]`
    /// darían lo mismo y un cambio real pasaría por «ya avisado».
    #[test]
    fn la_huella_ignora_el_orden_pero_no_los_bordes() {
        let a: Vec<String> = vec!["uno".into(), "dos".into()];
        let b: Vec<String> = vec!["dos".into(), "uno".into()];
        assert_eq!(huella(&a), huella(&b));

        assert_ne!(
            huella(&["ab".into(), "c".into()]),
            huella(&["a".into(), "bc".into()])
        );
        assert_ne!(huella(&[]), huella(&["".into()]));
    }

    /// **Un reloj que se atrasó no dispara un aviso.**
    ///
    /// Pasa de verdad: una placa sin batería arranca en 1970, y una resta con
    /// desbordamiento daría un número enorme y avisaría en cada corrida.
    #[test]
    fn un_reloj_atrasado_no_hace_avisar() {
        let h = huella(&["pacman 1 -> 2".into()]);
        let del_futuro = Recuerdo {
            huella: h.clone(),
            cuando: AHORA + 1_000_000,
        };
        assert_eq!(
            decidir(1, false, &h, &del_futuro, AHORA),
            Decision::YaAvisado
        );
    }
}
