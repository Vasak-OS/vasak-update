//! Lo único que toca el sistema: correr programas, leer y escribir archivos.
//!
//! Está separado por lo mismo que en el instalador: lo que decide se prueba
//! con texto escrito a mano, y lo que no se puede probar así queda acotado a
//! este archivo y se lee de una sentada.
//!
//! # Nada de esto necesita privilegios
//!
//! `checkupdates` copia la base de pacman a un directorio temporal y
//! sincroniza **ahí**, así que no toca `/var/lib/pacman` y corre como
//! cualquiera. `pacdiff --output` sólo lista. El espacio libre lo dice `df`.
//!
//! Este programa **no aplica** actualizaciones y no pide el candado de pacman.
//! Eso es a propósito: el día que exista la tienda, va a ser ella la que haga
//! las transacciones, y dos programas peleándose por `db.lck` dan un fallo que
//! no se entiende.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::analisis::{
    es_paquete_de_kernel, espacio_necesario_en_boot, mayor_initramfs, parsear_actualizaciones,
    parsear_pacnew, Actualizacion, Preflight,
};
use crate::aviso::Recuerdo;

const DIR_ARRANQUE: &str = "/boot";

/// Corre un programa y devuelve su salida estándar.
///
/// Un programa que no está no es un error: `pacman-contrib` y `pacdiff` son
/// dependencias del paquete, pero alguien las puede haber sacado. Vacío hace
/// que la comprobación diga «no encontré nada» en vez de romperse.
fn salida_de(programa: &str, args: &[&str]) -> String {
    Command::new(programa)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .map(|s| String::from_utf8_lossy(&s.stdout).into_owned())
        .unwrap_or_default()
}

pub fn pendientes() -> Vec<Actualizacion> {
    parsear_actualizaciones(&salida_de("checkupdates", &[]))
}

/// Cuáles de los que se actualizan son kernels.
///
/// Se pregunta por los archivos que **ya tienen instalados**: el paquete está
/// instalado —por eso se actualiza— así que es información local, sin red y
/// sin la base de archivos.
pub fn kernels_entre(actualizaciones: &[Actualizacion]) -> Vec<String> {
    actualizaciones
        .iter()
        .filter(|a| es_paquete_de_kernel(&salida_de("pacman", &["-Qlq", &a.nombre])))
        .map(|a| a.nombre.clone())
        .collect()
}

pub fn preflight(pendientes: &[Actualizacion]) -> Preflight {
    let kernels = kernels_entre(pendientes);
    let necesario = espacio_necesario_en_boot(!kernels.is_empty(), el_mayor_initramfs());
    Preflight::nuevo(
        pendientes.len(),
        kernels,
        parsear_pacnew(&salida_de("pacdiff", &["--output", "--nocolor"])),
        libre_en_boot(),
        necesario,
    )
}

/// El initramfs más grande de `/boot`, o una estimación si no se puede leer.
///
/// Se devuelve la estimación y no cero: cero diría «no hace falta espacio» y
/// callaría justo cuando no hay lugar. `/boot` con `fmask=0077` no lo lista
/// quien no es root, que es el caso normal para este programa.
fn el_mayor_initramfs() -> u64 {
    const ESTIMACION: u64 = 200 * 1024 * 1024;
    let Ok(entradas) = std::fs::read_dir(DIR_ARRANQUE) else {
        return ESTIMACION;
    };
    let archivos: Vec<(String, u64)> = entradas
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.metadata().map(|m| m.len()).unwrap_or(0),
            )
        })
        .collect();
    mayor_initramfs(&archivos).unwrap_or(ESTIMACION)
}

/// Lo libre en el sistema de archivos donde está `/boot`.
///
/// Se pregunta por `/boot` y no por `/`: son sistemas de archivos distintos, y
/// el que se llena es el chico.
fn libre_en_boot() -> u64 {
    salida_de("df", &["--output=avail", "-B1", DIR_ARRANQUE])
        .lines()
        .nth(1)
        .and_then(|l| l.trim().parse().ok())
        .unwrap_or(0)
}

/// Dónde se guarda lo que ya se avisó.
///
/// En el estado y no en la configuración: no es algo que nadie edite, es algo
/// que este programa recuerda. `XDG_STATE_HOME` existe para exactamente esto.
fn ruta_del_recuerdo() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("vasak-update/ultimo-aviso"))
}

/// Lo que se avisó la última vez.
///
/// Un archivo de dos líneas —huella y momento— y no JSON: son dos valores que
/// escribe y lee el mismo programa, y traer un formato para eso sería pagar
/// por nada. Cualquier cosa que no se entienda se trata como «nunca se avisó»,
/// que del lado seguro quiere decir avisar.
pub fn leer_recuerdo() -> Recuerdo {
    let Some(ruta) = ruta_del_recuerdo() else {
        return Recuerdo::default();
    };
    let Ok(texto) = std::fs::read_to_string(ruta) else {
        return Recuerdo::default();
    };
    let mut lineas = texto.lines();
    Recuerdo {
        huella: lineas.next().unwrap_or_default().trim().to_string(),
        cuando: lineas
            .next()
            .and_then(|l| l.trim().parse().ok())
            .unwrap_or(0),
    }
}

pub fn escribir_recuerdo(recuerdo: &Recuerdo) {
    let Some(ruta) = ruta_del_recuerdo() else {
        return;
    };
    if let Some(padre) = ruta.parent() {
        let _ = std::fs::create_dir_all(padre);
    }
    // Si falla no se aborta: lo peor que pasa es que mañana se vuelva a
    // avisar lo mismo, que es mucho mejor que no avisar.
    let _ = std::fs::write(ruta, format!("{}\n{}\n", recuerdo.huella, recuerdo.cuando));
}

pub fn ahora() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Muestra el aviso y devuelve el proceso que espera el botón.
///
/// Por `notify-send` y no hablando D-Bus desde acá: traer `zbus` para mandar
/// un mensaje una vez por día costaría más de un megabyte de binario y un
/// ejecutor. Un `fork` cuesta menos, y `libnotify` ya es dependencia del
/// escritorio.
///
/// `-A` implica esperar, así que este proceso vive lo que dure el cartel. Por
/// eso el tiempo es finito —uno que nunca expira sería un proceso que nunca
/// termina— y la unidad de systemd tiene además su propio límite.
///
/// **Se devuelve sin esperar** a propósito. Quien llama tiene que poder
/// anotar que ya avisó antes de ponerse a esperar: si el proceso se muere
/// mientras espera —el límite de systemd, cerrar la sesión, reiniciar— el
/// aviso ya salió pero no quedaría anotado, y mañana se avisaría lo mismo.
/// Que es exactamente lo que hay que evitar. Descubierto corriéndolo.
pub fn mostrar_aviso(titulo: &str, cuerpo: &str, boton: &str) -> Option<std::process::Child> {
    Command::new("notify-send")
        .args([
            "--app-name=VasakOS",
            "--icon=system-software-update",
            "--expire-time=60000",
            &format!("--action=abrir={boton}"),
            titulo,
            cuerpo,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Espera el botón y, si lo apretaron, abre la Configuración.
pub fn atender_boton(aviso: std::process::Child) {
    let Ok(salida) = aviso.wait_with_output() else {
        return;
    };
    if String::from_utf8_lossy(&salida.stdout).trim() != "abrir" {
        return;
    }
    // La sección la entiende `vasak-settings` por argumento; es la misma que
    // usa el menú del clic derecho del panel para llevar a un ajuste puntual.
    let _ = Command::new("vasak-settings")
        .arg("actualizaciones")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Escribe algo por la salida estándar sin morir si nadie la lee.
///
/// Un `println!` con la salida cerrada —`vasak-update --json | head -1`—
/// entra en pánico. Corriendo bajo systemd no pasa; corriendo a mano, sí.
pub fn escribir(texto: &str) {
    let _ = writeln!(std::io::stdout(), "{texto}");
}
