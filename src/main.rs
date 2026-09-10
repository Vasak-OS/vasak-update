//! Avisa cuando hay actualizaciones del sistema.
//!
//! # Qué es y qué no es
//!
//! **No actualiza nada.** Mira, avisa y contesta lo que le pregunten. Aplicar
//! es de la tienda, el día que exista: el candado de pacman —`db.lck`— admite
//! un solo dueño, y dos programas pidiéndolo a la vez dan un fallo que no se
//! entiende. Este programa nunca lo pide, así que no puede chocar con ella.
//!
//! # Por qué no hay ningún demonio
//!
//! Un proceso residente que duerme veintitrés horas y cincuenta y nueve
//! minutos para trabajar dos segundos es memoria ocupada a cambio de nada.
//! Esto es un programa de una sola pasada que arranca un temporizador de
//! systemd, hace lo suyo y se muere. El horario lo lleva systemd, que ya está
//! corriendo igual: no hay reloj propio, ni archivo de configuración con
//! intervalos, ni nada que despertar.
//!
//! Por lo mismo no hay ejecutor asíncrono ni cliente D-Bus propio: la
//! notificación sale por `notify-send`, que ya está instalado.
//!
//! # Cómo se usa
//!
//! ```text
//! vasak-update              comprueba y avisa si corresponde
//! vasak-update --json       lo que encontró, para la pantalla de Configuración
//! vasak-update --list       la lista, para una terminal
//! ```

mod analisis;
mod aviso;
mod sistema;

use aviso::{decidir, huella, Recuerdo};

const AYUDA: &str = "\
vasak-update — avisa cuando hay actualizaciones del sistema

    vasak-update            comprueba y avisa si corresponde
    vasak-update --json     lo que encontró, en JSON
    vasak-update --list     la lista, una por línea
    vasak-update --version

No aplica actualizaciones: para eso, `sudo pacman -Syu`.";

fn main() {
    match std::env::args().nth(1).as_deref() {
        None | Some("--check") => comprobar(),
        Some("--json") => en_json(),
        Some("--list") => listar(),
        Some("--version") | Some("-V") => {
            sistema::escribir(concat!("vasak-update ", env!("CARGO_PKG_VERSION")))
        }
        Some("--help") | Some("-h") => sistema::escribir(AYUDA),
        Some(otro) => {
            eprintln!("vasak-update: no entiendo «{otro}»\n\n{AYUDA}");
            std::process::exit(2);
        }
    }
}

/// La corrida del temporizador.
///
/// Lo caro se hace **después** de decidir: si no hay que avisar, no se
/// pregunta por los kernels —que es una llamada a `pacman -Qlq` por paquete—
/// ni por los `.pacnew`. La mayoría de las corridas terminan acá sin hacer
/// nada más que la comprobación.
fn comprobar() {
    let pendientes = sistema::pendientes();
    if pendientes.is_empty() {
        return;
    }

    let lineas: Vec<String> = pendientes
        .iter()
        .map(|a| format!("{} {} -> {}", a.nombre, a.version_vieja, a.version_nueva))
        .collect();
    let huella_actual = huella(&lineas);
    let recuerdo = sistema::leer_recuerdo();
    let ahora = sistema::ahora();

    // El kernel se consulta sólo si podría cambiar la decisión, o sea cuando
    // el conjunto es el mismo de antes. Si ya hay novedades, avisar no
    // depende de eso y la consulta sería trabajo tirado.
    let mismo_conjunto = huella_actual == recuerdo.huella;
    let kernels = if mismo_conjunto {
        sistema::kernels_entre(&pendientes)
    } else {
        Vec::new()
    };

    let decision = decidir(
        pendientes.len(),
        !kernels.is_empty(),
        &huella_actual,
        &recuerdo,
        ahora,
    );
    if !decision.avisa() {
        return;
    }

    let cuantas = pendientes.len();
    let titulo = if cuantas == 1 {
        "Hay 1 actualización".to_string()
    } else {
        format!("Hay {cuantas} actualizaciones")
    };
    let aviso = sistema::mostrar_aviso(&titulo, "Mirá qué cambia antes de aplicarlas.", "Ver");

    // Se anota **antes** de esperar el botón. El aviso ya salió; si este
    // proceso se muere esperando —el límite de la unidad, cerrar sesión,
    // reiniciar— el recuerdo se perdería y mañana se avisaría lo mismo, que
    // es justo lo que las reglas de `aviso` existen para evitar.
    sistema::escribir_recuerdo(&Recuerdo {
        huella: huella_actual,
        cuando: ahora,
    });

    if let Some(aviso) = aviso {
        sistema::atender_boton(aviso);
    }
}

fn listar() {
    for a in sistema::pendientes() {
        sistema::escribir(&format!(
            "{} {} -> {}",
            a.nombre, a.version_vieja, a.version_nueva
        ));
    }
}

/// Lo que consume la pantalla de Configuración.
///
/// Por la salida estándar de un programa y no por un servicio: la pantalla lo
/// pregunta cuando alguien la abre, o sea unas pocas veces por día. Un
/// servicio para eso sería un proceso vivo todo el tiempo para contestar cada
/// tanto.
fn en_json() {
    let pendientes = sistema::pendientes();
    let preflight = sistema::preflight(&pendientes);
    let salida = serde_json::json!({
        "pendientes": pendientes,
        "preflight": preflight,
    });
    sistema::escribir(&salida.to_string());
}
