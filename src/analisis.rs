//! Lo que hay que saber **antes** de aplicar una actualización.
//!
//! Nació en `vasak-settings` y vive acá, que es donde tiene que estar: la
//! comprobación periódica y la pantalla miran lo mismo, y dos copias del mismo
//! analizador son dos que se separan. La pantalla llama a este programa con
//! `--json` en vez de repetir la lógica.
//!
//! Un sistema rolling que no se actualiza durante meses es la forma más segura
//! de romperlo, así que el escritorio tiene que avisar. Pero avisar no alcanza:
//! `pacman -Syu` puede dejar el equipo peor de lo que estaba, y las maneras son
//! conocidas y pocas. Este módulo es la comprobación previa.
//!
//! # Qué mira, y por qué cada cosa
//!
//! **El espacio en `/boot`.** Es el que deja un sistema que **no arranca**: si
//! `pacman` se queda sin lugar a mitad de escribir el initramfs, el kernel
//! nuevo queda sin su initramfs y el viejo ya no está. Por eso el instalador le
//! da 1 GiB, y por eso esto se comprueba antes y no después.
//!
//! **Si cambia el kernel.** Decide si hay que reiniciar y si hace falta espacio
//! en `/boot`. Los módulos de una versión no los carga un kernel de otra, así
//! que hasta reiniciar no se puede enchufar nada nuevo.
//!
//! **Los `.pacnew`.** Un archivo de configuración que cambió upstream y quedó
//! al lado del nuestro sin aplicar. No rompe nada hoy y rompe algo dentro de
//! seis meses, cuando nadie se acuerde.
//!
//! # Por qué es todo función pura
//!
//! Lo que ejecuta los comandos vive en `commands/`; acá sólo se interpreta lo
//! que devolvieron. Es lo mismo que se hizo con los analizadores del
//! instalador, y por la misma razón: una decisión que se toma leyendo texto de
//! otro programa se tiene que poder probar con ese texto escrito a mano, sin
//! depender de qué actualizaciones haya hoy en el equipo que corre los tests.

use serde::Serialize;

/// Un paquete que se va a actualizar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Actualizacion {
    pub nombre: String,
    pub version_vieja: String,
    pub version_nueva: String,
}

/// Lee la salida de `checkupdates`, que es la de `pacman -Qu`.
///
/// El formato es `nombre vieja -> nueva`, una por línea. Se descarta todo lo
/// que no tenga esa forma en vez de intentar adivinar: la salida puede traer
/// avisos, líneas vacías, y —si alguien exporta `COLOR`— códigos de escape.
///
/// Los códigos de escape se sacan antes de partir. `checkupdates` los filtra
/// cuando escribe a una terminal, pero acá se lo llama con la salida
/// redirigida y basta con que `pacman.conf` tenga `Color` para que aparezcan.
/// Sin sacarlos, el nombre del primer paquete vendría con basura adelante y
/// ninguna comparación contra él funcionaría.
pub fn parsear_actualizaciones(salida: &str) -> Vec<Actualizacion> {
    salida
        .lines()
        .filter_map(|linea| {
            let limpia = sin_escapes(linea);
            let mut partes = limpia.split_whitespace();
            let nombre = partes.next()?;
            let vieja = partes.next()?;
            if partes.next()? != "->" {
                return None;
            }
            let nueva = partes.next()?;
            // Una quinta palabra quiere decir que esto no era lo que
            // parecía; se descarta en vez de quedarse con las primeras cuatro.
            if partes.next().is_some() || nombre.is_empty() {
                return None;
            }
            Some(Actualizacion {
                nombre: nombre.to_string(),
                version_vieja: vieja.to_string(),
                version_nueva: nueva.to_string(),
            })
        })
        .collect()
}

/// Saca los códigos de escape ANSI de una línea.
fn sin_escapes(linea: &str) -> String {
    let mut salida = String::with_capacity(linea.len());
    let mut chars = linea.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            salida.push(c);
            continue;
        }
        // `ESC [ ... letra`. Se descarta hasta la letra final inclusive.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    salida
}

/// Si un paquete es un kernel, mirando **qué archivos tiene**.
///
/// Por los archivos y no por el nombre. Un `nombre.starts_with("linux")`
/// parece razonable hasta que se lo mira de cerca: `linux-firmware` no es un
/// kernel y `linux-cachyos-bore` sí, y la lista de variantes no termina nunca
/// —cada sabor de cada derivada agrega la suya—. Lo que sí es invariable es
/// que un paquete de kernel instala su imagen en
/// `usr/lib/modules/<version>/vmlinuz`, que es de donde el hook de mkinitcpio
/// la saca.
///
/// Recibe la lista de archivos —lo que devuelve `pacman -Qlq`— y no el nombre,
/// para que la decisión se pueda probar sin tener el paquete instalado.
pub fn es_paquete_de_kernel(archivos: &str) -> bool {
    // Con el prefijo, no sólo el nombre del archivo. `usr/share/ejemplo/vmlinuz`
    // termina igual y no es un kernel: darlo por bueno haría pedir espacio en
    // `/boot` y avisar que hay que reiniciar por un paquete que no tiene nada
    // que ver.
    //
    // El `trim_end_matches('/')` es porque `pacman -Qlq` lista los directorios
    // con barra al final.
    archivos
        .lines()
        .map(|l| l.trim().trim_end_matches('/'))
        .any(|l| {
            l.trim_start_matches('/').starts_with("usr/lib/modules/") && l.ends_with("/vmlinuz")
        })
}

/// El veredicto de la comprobación previa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preflight {
    /// Cuántos paquetes se actualizan.
    pub paquetes: usize,
    /// Los kernels que cambian de versión, por nombre.
    pub kernels: Vec<String>,
    /// Archivos de configuración nuevos sin aplicar.
    pub pacnew: Vec<String>,
    /// Lo que hay libre en el sistema de archivos de `/boot`.
    pub boot_disponible_bytes: u64,
    /// Lo que la actualización va a necesitar ahí. Cero si no cambia el kernel.
    pub boot_necesario_bytes: u64,
    /// Si en `/boot` hay lugar para escribir el initramfs **con red**.
    ///
    /// No es «entra o no entra». mkinitcpio, cuando le sobra espacio, escribe
    /// a un temporal y renombra, así que un corte a mitad de una actualización
    /// de kernel deja el initramfs anterior entero. Cuando no le sobra,
    /// escribe encima del archivo: funciona igual, pero una interrupción deja
    /// el initramfs truncado y el sistema no arranca.
    ///
    /// O sea que esto avisa de un riesgo, no de una imposibilidad — y por eso
    /// el aviso tiene que decir eso y no «no hay espacio».
    ///
    /// Va calculado y no se recalcula en la pantalla, a propósito: una regla
    /// que dice si algo es riesgoso y vive en dos lados es una regla que se
    /// separa, y el lado que quede desactualizado es el que calla.
    pub hay_lugar_con_red: bool,
    /// Si hay que reiniciar después. Cambió el kernel: los módulos de una
    /// versión no los carga un kernel de otra, así que hasta reiniciar no se
    /// puede enchufar nada que necesite uno que todavía no esté cargado — una
    /// impresora, un teléfono, una tarjeta de red USB.
    pub pide_reinicio: bool,
}

impl Preflight {
    /// El veredicto, con las dos conclusiones ya sacadas.
    ///
    /// Es el único constructor para que no haya forma de armar un `Preflight`
    /// cuyas conclusiones no se sigan de sus números.
    pub fn nuevo(
        paquetes: usize,
        kernels: Vec<String>,
        pacnew: Vec<String>,
        boot_disponible_bytes: u64,
        boot_necesario_bytes: u64,
    ) -> Self {
        Self {
            paquetes,
            hay_lugar_con_red: boot_disponible_bytes >= boot_necesario_bytes,
            pide_reinicio: !kernels.is_empty(),
            kernels,
            pacnew,
            boot_disponible_bytes,
            boot_necesario_bytes,
        }
    }
}

/// Cuánto espacio libre hace falta en `/boot` para que mkinitcpio escriba
/// **de forma segura**.
///
/// El número no es una invención nuestra: es el criterio que usa mkinitcpio
/// para decidir cómo escribe (`/usr/bin/mkinitcpio`, en `build_image`):
///
/// ```text
/// (( $((curr_size + (curr_size/4))) < space_left_on_device )) && compressout="$out".tmp
/// ```
///
/// Con ese espacio escribe a `$out.tmp` y renombra, así que una interrupción
/// —un corte de luz, un apagón a mitad de una actualización de kernel— deja el
/// initramfs anterior entero. Sin ese espacio **escribe encima del archivo**:
/// funciona igual, pero si se corta a la mitad el initramfs queda truncado y
/// el sistema no arranca.
///
/// O sea que la comprobación no es «entra o no entra»: es «se puede escribir
/// con red o sin red».
///
/// # Lo que esto **no** es
///
/// La primera versión pedía un juego de archivos de kernel entero por cada
/// kernel que se actualiza, y estaba mal por sobrada: mkinitcpio no necesita
/// eso, y con dos o tres kernels en un `/boot` de 1 GiB habría frenado
/// actualizaciones que funcionan. Un preflight que bloquea lo que anda es peor
/// que no tenerlo, porque lo primero que se aprende es a saltearlo.
///
/// Y es por el initramfs más grande y no por la suma: los kernels se escriben
/// de a uno y cada renombre libera el anterior, así que el pico lo marca el
/// más grande, no el total.
pub fn espacio_necesario_en_boot(hay_kernels: bool, mayor_initramfs: u64) -> u64 {
    if !hay_kernels {
        return 0;
    }
    mayor_initramfs.saturating_add(mayor_initramfs / 4)
}

/// El initramfs más grande que hay en `/boot`.
///
/// Es el archivo que marca el pico de espacio durante una actualización de
/// kernel: mkinitcpio decide cómo escribir mirando **el tamaño del archivo que
/// está por reemplazar**, no el total de la partición.
///
/// El de respaldo es siempre el más grande —lleva todos los módulos, sin la
/// detección de hardware que achica al normal—, así que en la práctica es el
/// que sale de acá.
///
/// Recibe los archivos de `/boot` como `(nombre, bytes)` y devuelve `None` si
/// no hay ninguno: el que llama lo traduce a una estimación fija en vez de a
/// cero, porque cero diría «no hace falta espacio» y dejaría pasar justo la
/// actualización que no tiene lugar.
pub fn mayor_initramfs(archivos: &[(String, u64)]) -> Option<u64> {
    archivos
        .iter()
        .filter(|(nombre, _)| nombre.starts_with("initramfs-") && nombre.ends_with(".img"))
        .map(|(_, tamano)| *tamano)
        .max()
        .filter(|m| *m > 0)
}

/// Los archivos `.pacnew` y `.pacsave` que dejó pacman, de la salida de
/// `pacdiff --output`.
///
/// Una ruta por línea. Se filtra lo que no sea una ruta absoluta porque
/// `pacdiff` escribe también avisos por la salida estándar cuando no encuentra
/// una herramienta de comparación.
pub fn parsear_pacnew(salida: &str) -> Vec<String> {
    salida
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/') && (l.ends_with(".pacnew") || l.ends_with(".pacsave")))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La salida de `checkupdates` tal cual, con las tres formas que aparecen:
    /// un cambio de versión, uno de `pkgrel` y uno con época.
    const SALIDA: &str = "\
linux-cachyos 7.2.3-1 -> 7.2.4-1
pacman 7.0.0-8 -> 7.0.0-9
amd-ucode 1:20260810-2 -> 1:20260912-1
";

    #[test]
    fn se_leen_las_actualizaciones() {
        let a = parsear_actualizaciones(SALIDA);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].nombre, "linux-cachyos");
        assert_eq!(a[0].version_vieja, "7.2.3-1");
        assert_eq!(a[0].version_nueva, "7.2.4-1");
        // La época va pegada a la versión y no se toca.
        assert_eq!(a[2].version_vieja, "1:20260810-2");
    }

    /// **Sin actualizaciones no hay actualizaciones.**
    ///
    /// `checkupdates` sale con 2 y sin escribir nada. Un analizador que
    /// devolviera una entrada vacía haría que el aviso apareciera siempre.
    #[test]
    fn una_salida_vacia_no_inventa_nada() {
        assert!(parsear_actualizaciones("").is_empty());
        assert!(parsear_actualizaciones("\n\n  \n").is_empty());
    }

    /// **Lo que no tiene la forma esperada se descarta.**
    ///
    /// La salida puede traer avisos por la misma vía. Quedarse con las
    /// primeras palabras de un aviso daría un paquete inventado, y con eso el
    /// preflight consultaría `pacman -Qlq` por algo que no existe.
    #[test]
    fn lo_que_no_es_una_actualizacion_se_descarta() {
        let raro = "\
:: Sincronizando la base de datos de paquetes...
warning: config file /etc/pacman.conf, line 5: directive 'Foo' in section 'options' not recognized.
foo 1.0 => 2.0
sin flecha 1.0 2.0
demasiadas 1.0 -> 2.0 palabras
paquete 1.0 -> 2.0
";
        let a = parsear_actualizaciones(raro);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].nombre, "paquete");
    }

    /// **Los códigos de color no se cuelan en el nombre.**
    ///
    /// `checkupdates` los filtra cuando escribe a una terminal, y acá se lo
    /// llama con la salida redirigida: basta con que `pacman.conf` tenga
    /// `Color` para que aparezcan. Sin sacarlos, el nombre del paquete viene
    /// con basura adelante y ninguna comparación contra él funciona — ni la de
    /// kernel, ni la que muestra la pantalla.
    #[test]
    fn los_codigos_de_color_no_se_cuelan() {
        let coloreada = "\u{1b}[1mlinux\u{1b}[0m 7.2.3-1 -> \u{1b}[1;32m7.2.4-1\u{1b}[0m\n";
        let a = parsear_actualizaciones(coloreada);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].nombre, "linux");
        assert_eq!(a[0].version_nueva, "7.2.4-1");
    }

    /// **Un kernel se reconoce por dónde pone su imagen, no por su nombre ni
    /// por el nombre del archivo.**
    ///
    /// Lo primero es la diferencia entre funcionar en Arch y funcionar en
    /// cualquier derivada: `linux-cachyos-bore` es un kernel y
    /// `linux-firmware` no, y la lista de sabores no termina nunca.
    ///
    /// Lo segundo es que la ruta entera importa: un `vmlinuz` suelto en
    /// `usr/share` no es un kernel, y darlo por bueno haría pedir espacio en
    /// `/boot` y avisar que hay que reiniciar por un paquete cualquiera.
    #[test]
    fn un_kernel_se_reconoce_por_donde_pone_su_imagen() {
        let kernel = "\
usr/lib/modules/7.2.3-1-cachyos/
usr/lib/modules/7.2.3-1-cachyos/vmlinuz
usr/lib/modules/7.2.3-1-cachyos/pkgbase
";
        assert!(es_paquete_de_kernel(kernel));

        // `linux-firmware`: empieza con «linux» y no es un kernel.
        let firmware = "\
usr/lib/firmware/
usr/lib/firmware/amdgpu/
usr/lib/firmware/amdgpu/aldebaran_sos.bin
";
        assert!(!es_paquete_de_kernel(firmware));

        // Ni un paquete que apenas nombre la palabra.
        assert!(!es_paquete_de_kernel("usr/share/doc/foo/vmlinuz.txt\n"));
        assert!(!es_paquete_de_kernel("usr/bin/vmlinuz-tool\n"));
        assert!(!es_paquete_de_kernel(""));

        // Y tampoco un archivo que se llame igual pero esté en otro lado. El
        // nombre del archivo solo no alcanza: lo que define a un kernel es que
        // su imagen esté donde el hook de mkinitcpio la va a buscar.
        assert!(!es_paquete_de_kernel("usr/share/ejemplo/vmlinuz\n"));
        assert!(!es_paquete_de_kernel("opt/loquesea/vmlinuz\n"));
        // Con barra al principio, como lo lista `pacman -Qoq`, sí.
        assert!(es_paquete_de_kernel("/usr/lib/modules/6.1.0/vmlinuz\n"));
    }

    /// **Lo que marca el pico es el initramfs más grande.**
    ///
    /// Y no la suma ni el juego entero de un kernel: mkinitcpio mira el
    /// tamaño **del archivo que está por reemplazar** para decidir cómo
    /// escribirlo, y los kernels se escriben de a uno, liberando el anterior
    /// en cada renombre.
    #[test]
    fn el_pico_lo_marca_el_initramfs_mas_grande() {
        let mib = 1024 * 1024;
        let archivos: Vec<(String, u64)> = vec![
            ("vmlinuz-linux".into(), 20 * mib),
            ("initramfs-linux.img".into(), 80 * mib),
            ("initramfs-linux-fallback.img".into(), 500 * mib),
            ("vmlinuz-linux-lts".into(), 20 * mib),
            ("initramfs-linux-lts-fallback.img".into(), 120 * mib),
            ("intel-ucode.img".into(), 5 * mib),
            // Lo que no es un initramfs no cuenta, por grande que sea.
            ("grub".into(), 900 * mib),
        ];
        assert_eq!(mayor_initramfs(&archivos), Some(500 * mib));
    }

    /// **Sin initramfs no se dice «cero».**
    ///
    /// Cero querría decir «no hace falta espacio», que es la respuesta
    /// peligrosa: callaría justo cuando no hay lugar. El que llama lo traduce
    /// a una estimación fija.
    #[test]
    fn un_boot_ilegible_no_da_cero() {
        assert_eq!(mayor_initramfs(&[]), None);
        assert_eq!(
            mayor_initramfs(&[("grub".into(), 900), ("vmlinuz-linux".into(), 100)]),
            None,
            "un vmlinuz no es un initramfs"
        );
        assert_eq!(mayor_initramfs(&[("initramfs-linux.img".into(), 0)]), None);
    }

    /// **Los `.pacnew` se leen y los avisos no.**
    #[test]
    fn se_leen_los_pacnew() {
        let salida = "\
==> WARNING: no merge program found
/etc/pacman.conf.pacnew
/etc/ssh/sshd_config.pacnew
/etc/locale.gen.pacsave
esto no es una ruta
/etc/algo.conf
";
        let p = parsear_pacnew(salida);
        assert_eq!(
            p,
            [
                "/etc/pacman.conf.pacnew",
                "/etc/ssh/sshd_config.pacnew",
                "/etc/locale.gen.pacsave"
            ]
        );
    }

    /// **La comprobación de `/boot` avisa de un riesgo, no de un bloqueo.**
    ///
    /// Con espacio de sobra mkinitcpio escribe a un temporal y renombra: un
    /// corte a mitad deja el initramfs anterior entero. Sin espacio de sobra
    /// escribe encima, y una interrupción deja el initramfs truncado y el
    /// sistema sin arrancar.
    #[test]
    fn el_veredicto_dice_lo_que_frena_y_lo_que_avisa() {
        let necesario = espacio_necesario_en_boot(true, 240 * 1024 * 1024);
        // El criterio de mkinitcpio: el archivo más 1/4.
        assert_eq!(necesario, 300 * 1024 * 1024);

        let con = |disponible| {
            Preflight::nuevo(
                40,
                vec!["linux".into()],
                vec!["/etc/pacman.conf.pacnew".into()],
                disponible,
                necesario,
            )
        };

        assert!(con(400 * 1024 * 1024).hay_lugar_con_red);
        // Justo al borde alcanza: lo que hace falta es lo que hace falta.
        assert!(con(necesario).hay_lugar_con_red);
        assert!(!con(necesario - 1).hay_lugar_con_red);
        assert!(con(necesario).pide_reinicio);

        // Sin cambio de kernel no hace falta espacio ni reiniciar, aunque se
        // actualicen cuarenta paquetes.
        let sin_kernel = Preflight::nuevo(
            40,
            Vec::new(),
            Vec::new(),
            0,
            espacio_necesario_en_boot(false, 240 * 1024 * 1024),
        );
        assert!(!sin_kernel.pide_reinicio);
        assert_eq!(sin_kernel.boot_necesario_bytes, 0);
        assert!(
            sin_kernel.hay_lugar_con_red,
            "sin kernel no hay nada que escribir"
        );
    }

    /// **Contra el pacman de este equipo: el formato es el que creemos.**
    ///
    /// Los tests de arriba usan salidas escritas a mano, así que codifican lo
    /// que suponemos del formato. Éste se lo pregunta a `pacman`: si algún día
    /// cambia cómo escribe `-Qu`, o cómo lista archivos, acá se ve — y no en
    /// el equipo de alguien con el aviso de actualizaciones mudo.
    ///
    /// Se saltea donde no haya `pacman`, que es cualquier contenedor de
    /// integración.
    #[test]
    fn el_formato_de_pacman_es_el_que_se_espera() {
        use std::process::Command;

        let corre = |args: &[&str]| -> Option<String> {
            let s = Command::new("pacman").args(args).output().ok()?;
            s.status
                .success()
                .then(|| String::from_utf8_lossy(&s.stdout).into_owned())
        };
        let Some(_) = corre(&["-V"]) else {
            eprintln!("pacman no está: se saltea");
            return;
        };

        // Que la lista de archivos de un kernel instalado lo delate. Se busca
        // el kernel por `/usr/lib/modules/<x>/vmlinuz`, que existe en
        // cualquier Arch, y se le pregunta a pacman de quién es.
        let Some(imagen) = std::fs::read_dir("/usr/lib/modules")
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("vmlinuz"))
            .find(|p| p.exists())
        else {
            eprintln!("sin kernels en /usr/lib/modules: se saltea");
            return;
        };
        let Some(duenio) = corre(&["-Qoq", &imagen.to_string_lossy()]) else {
            return;
        };
        let duenio = duenio.trim();
        assert!(!duenio.is_empty(), "nadie es dueño de {imagen:?}");

        let archivos = corre(&["-Qlq", duenio]).unwrap_or_default();
        assert!(
            es_paquete_de_kernel(&archivos),
            "{duenio} tiene {imagen:?} y no se lo reconoce como kernel"
        );

        // Y que un paquete que no lo es, no lo sea. `pacman` está siempre.
        let de_pacman = corre(&["-Qlq", "pacman"]).unwrap_or_default();
        assert!(!de_pacman.is_empty());
        assert!(!es_paquete_de_kernel(&de_pacman), "pacman no es un kernel");

        // El formato de `-Qu`: si hay actualizaciones pendientes en este
        // equipo, tienen que parsearse todas. Si no hay, no hay nada que
        // comprobar y tampoco es un fallo.
        if let Some(pendientes) = corre(&["-Qu"]) {
            let lineas = pendientes.lines().filter(|l| !l.trim().is_empty()).count();
            let leidas = parsear_actualizaciones(&pendientes).len();
            assert_eq!(
                leidas, lineas,
                "pacman -Qu escribió {lineas} líneas y se leyeron {leidas}:\n{pendientes}"
            );
        }
    }

    /// **El espacio que se pide es el criterio de mkinitcpio, no un invento.**
    ///
    /// `curr_size + curr_size/4 < space_left` es literalmente lo que mira
    /// mkinitcpio para decidir si escribe a un temporal o encima del archivo.
    /// Pedir más —un juego de kernel entero, que fue la primera versión—
    /// frenaría actualizaciones que funcionan, y un preflight que bloquea lo
    /// que anda es peor que no tenerlo.
    #[test]
    fn el_espacio_que_se_pide_es_el_de_mkinitcpio() {
        let mib = 1024 * 1024;
        assert_eq!(espacio_necesario_en_boot(false, 240 * mib), 0);
        assert_eq!(espacio_necesario_en_boot(true, 240 * mib), 300 * mib);
        assert_eq!(espacio_necesario_en_boot(true, 0), 0);
        // No desborda con números absurdos, que darían un valor chico y
        // dejarían pasar el caso riesgoso.
        assert_eq!(espacio_necesario_en_boot(true, u64::MAX), u64::MAX);
    }
}
