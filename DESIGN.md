---
name: El rastro
description: Consola web auto-hosted para construir, validar y versionar puentes y bloques de integración
colors:
  papel: "#f5f6f3"
  superficie: "#ffffff"
  tinta: "#14352d"
  tinta-suave: "#5b7169"
  tinta-tenue: "#64706b"
  borde: "#e1e5df"
  borde-fuerte: "#cdd3cc"
  ok: "#1e7a4e"
  ok-fondo: "#e4f0e9"
  mal: "#b3402e"
  mal-fondo: "#f7e6e2"
  aviso: "#855d0e"
  aviso-fondo: "#f4ebd7"
  info: "#2b6cb0"
  info-fondo: "#e4edf8"
typography:
  display:
    fontFamily: "Schibsted Grotesk, system-ui, sans-serif"
    fontSize: "21px"
    fontWeight: 700
    lineHeight: 1.3
  title:
    fontFamily: "Schibsted Grotesk, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "Schibsted Grotesk, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Schibsted Grotesk, system-ui, sans-serif"
    fontSize: "12.5px"
    fontWeight: 500
    lineHeight: 1.4
  mono:
    fontFamily: "Spline Sans Mono, ui-monospace, SF Mono, Menlo, monospace"
    fontSize: "12.5px"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  panel: "10px"
  control: "7px"
  chip: "4px"
spacing:
  tight: "8px"
  base: "12px"
  field: "16px"
components:
  button-primary:
    backgroundColor: "{colors.tinta}"
    textColor: "{colors.papel}"
    rounded: "{rounded.control}"
  button-secondary:
    backgroundColor: "{colors.superficie}"
    textColor: "{colors.tinta}"
    rounded: "{rounded.control}"
  button-danger:
    backgroundColor: "{colors.mal}"
    textColor: "{colors.superficie}"
    rounded: "{rounded.control}"
  panel:
    backgroundColor: "{colors.superficie}"
    rounded: "{rounded.panel}"
  input:
    backgroundColor: "{colors.superficie}"
    rounded: "{rounded.control}"
---

# Design System: El rastro

## Overview

**Creative North Star: "El rastro de tinta"**

El rastro es la consola de un terreno acotado: papel hueso (#f5f6f3) sobre el que se dibuja con una tinta verde petróleo (#14352d); el verde es la única marca. El color no decora — **acota información**: cuatro tintas de estado (verde ok, ladrillo mal, ámbar aviso, azul info) funcionan como las acotaciones de un mapa topográfico, y nada más tiene croma. La voz del sistema es **precisa, sobria y operativa**: la interfaz es la pieza que menos se nota de la plataforma; lo que se nota es el rastro que cada puente deja al ejecutarse.

Todo lo que una máquina produjo se imprime en mono (slugs, refs `$.`, estados); todo lo que dijeron personas va en la grotesca. Profundidad por línea, no por sombra; movimiento solo como respuesta al usuario.

**Key Characteristics:**
- Cromo binario papel/tinta con estados como única acotación de color.
- Dos familias autoalojadas: Schibsted Grotesk (interfaz) y Spline Sans Mono (valores de máquina).
- Metáfora literalizada del producto: la cadena es un rastro de raíl y nudos.
- Sin movimiento automático; guardia activa con `prefers-reduced-motion`.
- Superficies planas con borde de línea; la sombra es excepción (dropdown, tour).

## Colors

Paleta de **mapa topográfico acotado**: papel y tintas neutras dibujan el terreno, y solo los estados llevan color.

### Primary
- **Tinta petróleo** (#14352d): la marca entera. Botón primario, sidebar, texto de primer nivel, foco. No se usa como fondo de mensajes ni como titular de estados — para eso están las acotaciones.

### Neutral
- **Papel hueso** (#f5f6f3): fondo de la aplicación (el mapa).
- **Superficie** (#ffffff): paneles y controles (la hoja sobre el mapa).
- **Tinta suave** (#5b7169): texto secundario y prosa de ayuda.
- **Tinta tenue** (#64706b): texto terciario, placeholders, metadatos.
- **Borde** (#e1e5df) / **Borde fuerte** (#cdd3cc): líneas de separación y controles, respectivamente.

### Estados (las acotaciones)
- **Verde de estado** (#1e7a4e, fondo #e4f0e9): éxito de ejecución o paso, avisos informativos de operación completada.
- **Ladrillo de estado** (#b3402e, fondo #f7e6e2): fallos, errores de validación, y la variante `peligro` de botón (destruye o publica irreversible).
- **Ámbar de estado** (#855d0e, fondo #f4ebd7): pendientes, `skipped`, estados intermedios — y en `status` nunca significa error.
- **Azul de estado** (#2b6cb0, fondo #e4edf8): ejecución en curso (`running`), información desplegable.

### Named Rules
**The Only-Data-Color Rule.** El color significa datos: estado de ejecución o de paso, y la variante de peligro en acciones irreversibles. Nada del cromo (papel, tinta, bordes) tiene color; nada de los estados decora. Si un elemento no informa de un estado, no lleva croma.

**The One Terrain Rule.** Petróleo (#14352d) aparece en ≤2 roles por pantalla (marca+acción/foco). Su riqueza relaja la demanda de más color; la acotación de estados es el único bono.

## Typography

**Interfaz:** Schibsted Grotesk (400–700, autoalojada, fallback system-ui)
**Valores de máquina:** Spline Sans Mono (400–600, autoalojada, fallback ui-monospace)

**Carácter:** pareja de cuaderno de campo — una grotesca humanista que no grita para la prosa, una mono compacta y legible para lo que una máquina escribió.

### Hierarchy
- **Display** (700, 21px, 1.3): título de página de listado.
- **Headline** (600–700, 19px, mono en puentes/bloques): título de detalle — en el editor es el slug, un valor de máquina.
- **Title** (600, 14px, 1.4): título de panel ("Trigger", "Cadena").
- **Body** (400, 13.5px, 1.55): prosa, descripciones, mensajes de aviso.
- **Label** (500, 12.5px, 1.4): etiquetas de campo y controles.
- **Machine value** (mono 400/600, 11–12.5px): slugs, refs `$.`, estados, durationMs, versiones.

### Named Rules
**The Machine-Value Rule.** La mono imprime exclusivamente valores de máquina: slugs, refs, estados, números de versión. En prosa y etiquetas, siempre la grotesca. Si un mono empieza a explicar en frases, es prosa mal disfrazada.

## Layout

Una columna de paneles (`space-y-3`, 12px de ritmo) con páginas de máximo ~1000px funcionales; dentro del panel, 16px (`p-4`) de aire y grillas `md:grid-cols-2` para campos afines (la fila input + timeoutMs acota a `1fr 150px`). Ritmo interior: 8px entre concepto, 12px entre campo, 16px entre bloques. Responsive: `md` (~768px) dobla columnas; en móvil todo va a una columna y el nombre del puente es contexto central. La densidad es de panel operativo — compacta, no apretada.

## Elevation & Depth

Sistema **plano por defecto**: la profundidad la dibujan líneas (bordes de 1px) y contraste papel/superficie, no sombras. Dos excepciones deliberadas: la sombra del dropdown del autocompletado (elevación temporal de menú) y la del popover del tour (`0 12px 32px rgb(20 53 45 / 0.18)` — el único difuminado del sistema, reservado a la capa guiada).

### Named Rules
**The Line-Depth Rule.** Si la profundidad no responde a una capa temporal (menú abierto, popover de guía), se dibuja con borde, no con sombra.

## Shapes

Esquinas suaves en dos pasos: paneles `10px`, controles y campos `7px`, chips/insignias `4px` (esquinas casi rectas para valores de máquina). Bordes de 1px en `borde` para separar y `borde-fuerte` para controles; el borde que comunica estado usa `border-mal`. La geometría del tour recorta sin redondeo (`border-radius: 0`). Silueta recurrente: el nudo circular de 16px sobre el raíl de 2px — el marcador del rastro.

## Components

### Botones
- **Shape:** 7px, altura 34px (2px py + 13.5px texto) o 28px (`chico`).
- **Primario:** fondo `tinta` (#14352d), texto `papel`, `px-3.5 py-2` — una sola acción primaria por página.
- **Secundario:** superficie con borde `borde-fuerte`, hover hacia borde más oscuro.
- **Peligro:** fondo `mal` (#b3402e), texto superficie — solo destruye o publica irreversible (	RTCK del kit: el color sí significa).
- **Focus:** outline 2px `tinta` con offset 2px, siempre visible.

### Insignias de estado
- **Style:** mono 11px, LED redondo de 1.5px + texto de estado (`succeeded`, `failed`, `running`…); fondo del par estado/fondo del sistema.
- **State:** el color ES el dato; sin estados no hay insignia (el tour no las usa).

### Panel
- **Corner:** 10px; fondo superficie sobre papel; borde `borde`.
- **Shadow Strategy:** ninguna (ver Elevation).
- **Internal Padding:** 16px; título de `title` (600, 14px) arriba.

### Inputs / Campos
- **Style:** superficie, borde `borde-fuerte`, 7px; mono para valores de máquina.
- **Focus:** borde → `tinta-suave` + outline 2px `tinta`.
- **Error:** nota en línea de 12.5px con fondo `mal/5` y borde `mal/40` bajo el campo; `aria-invalid` cuando aplica.
- **Campo etiqueta:** siempre visible con ayuda de 12px `tinta-tenue`.

### Navegación (sidebar)
- **Style:** barra lateral en tinta (#14352d) de texto claro; item activo diferenciado por peso/fondo sutil, no por color de estado.

### El rastro (componente firma)
La cadena de bloques se pinta como su metáfora: un raíl `border-l-2` que recorre las instancias y un nudo circular de 16px por paso — verde de estado al pasar, ladrillo con fondo `mal-fondo` al fallar ("border-mal bg-mal-fondo"). Es a la vez timeline y mapa de errores: dónde falla la cadena se ve sin leer un mensaje. En resumen colapsada, la fila muestra nº, nombre en mono, insignia `when`, cuenta de errores y plegador; en las ejecuciones, el mismo rastro se lee paso a paso con durationMs.

## Do's and Don'ts

### Do:
- **Do** imprimir en mono todo valor de máquina (slug, ref `$.`, estado, versión, durationMs) y en grotesca toda prosa.
- **Do** usar el par estado/fondo exacto para cada estado (ok #1e7a4e/#e4f0e9, mal #b3402e/#f7e6e2, aviso #855d0e/#f4ebd7, info #2b6cb0/#e4edf8).
- **Do** anunciar lo irreversible con variante `peligro` (ladrillo) — color = consecuencia.
- **Do** mantener foco visible de 2px `tinta` con offset en todo control enfocable.
- **Do** respetar `prefers-reduced-motion` (dura 0.01ms) — las animaciones solo responden al usuario.

### Don't:
- **Don't** usar cromo (papel/tinta) para comunicar estados ni estados como decoración (color solo = datos).
- **Don't** añadir sombras a superficies estáticas: profundidad por línea salvo dropdown/tour.
- **Don't** meter animaciones automáticas (pulsos decorativos, marquee): sin estado no hay movimiento.
- **Don't** cargar fuentes por CDN: archivos self-hosted en `console/src/fonts/`.
- **Don't** redactar mono en más de una frase: es un valor de máquina, no prosa.
