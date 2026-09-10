# vasak-update

Avisa cuando hay actualizaciones del sistema, y qué conviene mirar antes de
aplicarlas.

## Qué es y qué no es

**No actualiza nada.** Mira, avisa, y contesta lo que le pregunten.

Aplicar va a ser de la tienda, el día que exista. El candado de pacman
—`/var/lib/pacman/db.lck`— admite un solo dueño: dos programas pidiéndolo a la
vez dan un fallo que no se entiende y que aparece a mitad de una
actualización. Este programa **nunca lo pide**, así que no puede chocar con
ella.

## Cómo se usa

```
vasak-update              comprueba y avisa si corresponde
vasak-update --json       lo que encontró, para la pantalla de Configuración
vasak-update --list       la lista, una por línea
```

Normalmente no se lo llama a mano: lo corre un temporizador de systemd, una
vez por día.

## Qué mira antes

| | por qué |
|---|---|
| espacio en `/boot` | es el único de la lista que deja un equipo que **no arranca** |
| si cambia el kernel | hay que reiniciar, y hasta entonces no carga ningún módulo nuevo |
| `.pacnew` pendientes | no rompen nada hoy; se descubren meses después |

Lo del espacio es un aviso de **riesgo**, no de imposibilidad. mkinitcpio,
cuando le sobra lugar, escribe el arranque a un temporal y lo renombra: si se
corta la luz, el anterior queda entero. Cuando no le sobra escribe encima, y
una interrupción lo deja a medio escribir. El umbral es el suyo, no uno
inventado: `curr_size + curr_size/4 < space_left`, de `build_image`.

## Cuándo avisa, y cuándo se calla

Un aviso diario e idéntico enseña a ignorar los avisos, y a partir de ahí el
que importa tampoco se lee. Las reglas:

1. Sin actualizaciones, nada.
2. Si es exactamente lo mismo que la última vez, nada.
3. Salvo que cambie el kernel, que avisa igual.
4. O que hayan pasado siete días desde el último aviso.

## Por qué no hay ningún demonio

Un proceso residente que duerme veintitrés horas y cincuenta y nueve minutos
para trabajar dos segundos es memoria ocupada a cambio de nada.

Esto es un programa de una sola pasada: arranca, hace lo suyo y se muere. El
horario lo lleva systemd, que ya está corriendo igual — no hay reloj propio,
ni archivo de configuración con intervalos, ni nada que despertar. Por lo
mismo no hay ejecutor asíncrono ni cliente D-Bus: la notificación sale por
`notify-send`, que ya está instalado.

Dos dependencias de Rust, y el binario pesa unos 380 KB.

## Configurarlo

Desde **Configuración → Actualizaciones**, o a mano:

```
systemctl --user disable --now vasak-update.timer   # apagarlo
systemctl --user enable  --now vasak-update.timer   # encenderlo
```

Para cambiar cada cuánto, un archivo en
`~/.config/systemd/user/vasak-update.timer.d/`:

```ini
[Timer]
OnUnitActiveSec=1w
```

## Nada de esto necesita privilegios

`checkupdates` copia la base de pacman a un directorio temporal y sincroniza
**ahí**, así que no toca `/var/lib/pacman`. `pacdiff --output` sólo lista. El
espacio libre lo dice `df`.
