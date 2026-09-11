//! Por qué no se pudo comprobar, y qué hacer al respecto.
//!
//! # El agujero que esto tapa
//!
//! `checkupdates` sale con **2** cuando no hay actualizaciones y con **1**
//! cuando no pudo averiguarlo. La primera versión de este programa miraba sólo
//! la salida estándar, que en los dos casos está vacía: una clave de firma
//! vencida o un espejo a medio sincronizar se veían exactamente igual que «el
//! sistema está al día», y el aviso callaba **para siempre** sin que nadie se
//! enterara.
//!
//! Es el peor fallo posible en un programa cuyo único trabajo es avisar.
//!
//! # Por qué acá hay un clasificador y no un mensaje de error
//!
//! Los tres fallos que se ven en la práctica tienen salida conocida, y hoy hay
//! que buscarla en el wiki. Mostrar la salida cruda de pacman a quien no abre
//! una terminal no le sirve de nada: lo que sirve es «se venció una clave de
//! firma, esto lo arregla».
//!
//! Lo que no se reconoce **no se disfraza**: se dice que no se pudo comprobar
//! y se muestra lo que dijo pacman. Inventar una explicación es peor que no
//! dar ninguna.

use serde::Serialize;

/// Por qué no se pudo comprobar si hay actualizaciones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "causa", content = "detalle")]
pub enum Fallo {
    /// No hay red. Es el caso más común y el único que se arregla solo.
    SinRed,
    /// Una clave de firma desconocida o vencida. Pasa cuando el equipo estuvo
    /// mucho tiempo sin actualizar: las claves del llavero caducan.
    Firma,
    /// Un espejo que contesta pero le falta algo, o la base local quedó a
    /// medio sincronizar.
    Espejo,
    /// Quedó el candado de una ejecución anterior de pacman.
    Candado,
    /// Sin espacio para bajar la base de paquetes.
    SinEspacio,
    /// Algo que no se reconoce. Se lleva el texto crudo: inventar una
    /// explicación es peor que no dar ninguna.
    Desconocido(String),
}

impl Fallo {
    /// Si hace falta que alguien haga algo.
    ///
    /// De esto depende si se avisa. Quedarse sin red un rato no merece un
    /// cartel —se arregla solo, y avisar de cada corte enseña a ignorar los
    /// avisos—; una clave vencida sí, porque hasta que alguien la arregle el
    /// equipo no se entera de ninguna actualización.
    pub fn pide_accion(&self) -> bool {
        !matches!(self, Fallo::SinRed)
    }

    /// Qué decirle a quien no abre una terminal.
    pub fn explicacion(&self) -> &'static str {
        match self {
            Fallo::SinRed => "No se pudo comprobar porque no hay conexión.",
            Fallo::Firma => {
                "Se venció una clave de firma. Hasta arreglarlo, el equipo no se entera de \
                 ninguna actualización."
            }
            Fallo::Espejo => {
                "El servidor de paquetes contestó a medias. Suele ser pasajero; si sigue, \
                 conviene rehacer la lista de servidores."
            }
            Fallo::Candado => {
                "Quedó una actualización a medio empezar. Hay que comprobar que no haya \
                 ninguna en curso antes de seguir."
            }
            Fallo::SinEspacio => "No hay espacio para bajar la lista de paquetes.",
            Fallo::Desconocido(_) => "No se pudo comprobar si hay actualizaciones.",
        }
    }

    /// El comando que lo arregla, si hay uno.
    ///
    /// Se muestra el comando y no se lo ejecuta: todos piden privilegios, y
    /// este programa no los usa nunca. Y el del candado **no puede**
    /// ejecutarse solo — borrar el candado con una actualización de verdad en
    /// curso corrompe la base de paquetes.
    pub fn arreglo(&self) -> Option<&'static str> {
        match self {
            Fallo::SinRed => None,
            Fallo::Firma => Some("sudo pacman -Sy archlinux-keyring vasakos-keyring"),
            Fallo::Espejo => Some("sudo pacman -Syy"),
            Fallo::Candado => Some("sudo rm /var/lib/pacman/db.lck"),
            Fallo::SinEspacio => None,
            Fallo::Desconocido(_) => None,
        }
    }
}

/// Qué salió mal, a partir de lo que escribió pacman.
///
/// El orden importa y no es alfabético: se mira primero lo que tiene una
/// salida concreta. Un equipo sin red **también** informa «failed retrieving
/// file», así que buscar el espejo antes que la red diría «rehacé la lista de
/// servidores» a quien lo único que tiene que hacer es enchufar el cable.
pub fn clasificar(texto: &str) -> Fallo {
    let t = texto.to_ascii_lowercase();

    let contiene = |agujas: &[&str]| agujas.iter().any(|a| t.contains(a));

    if contiene(&[
        "could not resolve host",
        "temporary failure in name resolution",
        "network is unreachable",
        "no address record",
        "resolving timed out",
        "couldn't connect to server",
    ]) {
        return Fallo::SinRed;
    }
    if contiene(&[
        "unknown trust",
        "signature from",
        "invalid or corrupted package (pgp",
        "key.*is unknown",
        "marginal trust",
        "keyring",
    ]) {
        return Fallo::Firma;
    }
    if contiene(&["unable to lock database", "db.lck"]) {
        return Fallo::Candado;
    }
    if contiene(&["not enough free disk space", "no space left on device"]) {
        return Fallo::SinEspacio;
    }
    if contiene(&[
        "failed retrieving file",
        "failed to update",
        "cannot fetch updates",
        "404",
        "the requested url returned error",
    ]) {
        return Fallo::Espejo;
    }
    Fallo::Desconocido(primera_linea_util(texto))
}

/// La primera línea que diga algo, para el caso desconocido.
///
/// pacman escribe avisos antes del error de verdad —de `pacman.conf`, de
/// espejos lentos— y la primera línea a secas suele ser uno de ésos. Se
/// prefiere la que empieza con `error:`, que es la que explica.
fn primera_linea_util(texto: &str) -> String {
    let lineas: Vec<&str> = texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lineas
        .iter()
        .find(|l| l.starts_with("error:"))
        .or_else(|| lineas.last())
        .map(|l| l.chars().take(200).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Sin red se dice sin red, aunque pacman hable de archivos.**
    ///
    /// Es el orden que importa: un equipo sin red informa también «failed
    /// retrieving file», y clasificarlo como espejo le diría «rehacé la lista
    /// de servidores» a quien sólo tiene que enchufar el cable.
    #[test]
    fn sin_red_gana_sobre_el_espejo() {
        let salida = "\
error: failed retrieving file 'core.db' from mirror.example.org : Could not resolve host: mirror.example.org
error: failed to synchronize all databases (unexpected error)";
        assert_eq!(clasificar(salida), Fallo::SinRed);
        assert!(
            !clasificar(salida).pide_accion(),
            "un corte de red no merece un cartel"
        );
    }

    /// **Una clave vencida se reconoce y trae su arreglo.**
    ///
    /// Es el fallo que deja al equipo sin enterarse de nada hasta que alguien
    /// lo toca, así que es el que más importa nombrar bien.
    #[test]
    fn una_clave_vencida_trae_su_arreglo() {
        let salida = "\
error: core: signature from \"Arch Linux <pierre@archlinux.de>\" is unknown trust
error: failed to update core (invalid or corrupted database (PGP signature))";
        let f = clasificar(salida);
        assert_eq!(f, Fallo::Firma);
        assert!(f.pide_accion());
        assert!(f.arreglo().unwrap().contains("keyring"));
    }

    /// **El candado se reconoce, y su arreglo no se ejecuta solo.**
    ///
    /// Borrar el candado con una actualización de verdad en curso corrompe la
    /// base de paquetes. Se muestra el comando; apretarlo es de la persona.
    #[test]
    fn el_candado_se_reconoce_y_no_se_toca() {
        let salida = "error: unable to lock database\nerror: failed to init transaction (unable to lock database)";
        assert_eq!(clasificar(salida), Fallo::Candado);
        assert_eq!(
            Fallo::Candado.arreglo(),
            Some("sudo rm /var/lib/pacman/db.lck")
        );
    }

    #[test]
    fn el_espejo_y_el_espacio() {
        assert_eq!(
            clasificar("error: failed retrieving file 'extra.db' from mirror : The requested URL returned error: 404"),
            Fallo::Espejo
        );
        assert_eq!(
            clasificar("error: Partition /var too full: 12 blocks needed, 3 blocks free\nno space left on device"),
            Fallo::SinEspacio
        );
    }

    /// **Lo que no se reconoce no se disfraza.**
    ///
    /// Inventar una explicación es peor que no dar ninguna: manda a alguien a
    /// arreglar lo que no está roto. Se muestra lo que dijo pacman.
    #[test]
    fn lo_desconocido_se_dice_tal_cual() {
        let salida = "warning: config file /etc/pacman.conf, line 5: directive 'Foo' not recognized.\nerror: algo rarísimo pasó acá";
        match clasificar(salida) {
            Fallo::Desconocido(texto) => {
                assert!(texto.contains("algo rarísimo"), "{texto}");
                assert!(
                    !texto.contains("warning"),
                    "se quedó con el aviso y no con el error: {texto}"
                );
            }
            otro => panic!("se esperaba Desconocido y salió {otro:?}"),
        }
        assert_eq!(Fallo::Desconocido(String::new()).arreglo(), None);
    }

    /// **Un texto vacío no entra en pánico ni inventa nada.**
    #[test]
    fn un_texto_vacio_no_rompe() {
        assert_eq!(clasificar(""), Fallo::Desconocido(String::new()));
        assert_eq!(clasificar("\n\n   \n"), Fallo::Desconocido(String::new()));
    }

    /// **Y el texto del caso desconocido no crece sin límite.**
    ///
    /// Va a una notificación y a una pantalla. Un volcado de cien líneas no
    /// entra en ninguna de las dos y tapa lo que sí importa.
    #[test]
    fn el_texto_desconocido_esta_acotado() {
        let largo = format!("error: {}", "x".repeat(5000));
        match clasificar(&largo) {
            Fallo::Desconocido(texto) => assert!(texto.chars().count() <= 200, "{}", texto.len()),
            otro => panic!("{otro:?}"),
        }
    }
}
