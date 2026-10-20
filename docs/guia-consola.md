# Guía de la consola

Todo lo que puedes hacer desde `/console`, quién puede hacer cada cosa y
cómo se hacen. La consola no inventa reglas: cada botón que ves es una
acción que el plano admin permite a tu rol — y si algo se te escapa, el
error del servidor se muestra donde ocurre.

## Entrar

- **Email y contraseña** (acceso por persona): si perteneces a un solo
  workspace, entras directamente; si perteneces a varios, se te pedirá
  elegir uno — la sesión queda fijada a ese workspace.
- **API key** (acceso de automatización/demo): actúa como `owner`.
- **Salir** revoca la sesión en el servidor (con sesión de usuario): el
  token muere, no solo desaparece de tu navegador.

Tu rol aparece en la cabecera y define qué botones ves:

| Rol | Puede |
|---|---|
| `viewer` | Ver todo (ejecuciones, puentes, bloques, credenciales, miembros, métricas) y exportar. Nada de escritura. |
| `runner` | Además: disparar puentes, re-ejecutar (replay) y ensayar (dry-run). |
| `editor` | Además: crear/editar puentes y bloques, publicar versiones, rotar/crear credenciales, rollback, importar/duplicar. |
| `owner` | Además: gestionar miembros (añadir, cambiar rol, expulsar). |

Un cambio de rol o una expulsión surte efecto **al instante** — no hay
que cerrar sesión.

## El tour de bienvenida

La consola se explica sola la primera vez: al entrar, un tour guiado
señala el shell (navegación, tu rol, salir), el dashboard de Ejecuciones
con su **rastro en miniatura** y el trace paso a paso, y las superficies
que tu rol tiene delante.

- **Arranca solo la primera vez** en ese navegador. Después ya no vuelve
  a salir: el registro «ya lo vi» vive **solo en tu navegador**
  (`localStorage`, una entrada por workspace y versión del tour). Si
  borras los datos del sitio, volverás a verlo.
- **Ver tutorial** (en el sidebar, junto a tu rol y *Salir*; en móvil, en
  la barra superior) lo relanza **desde el primer paso** siempre que
  quieras, esté o no marcado como visto.
- **Se adapta a tu rol**: `viewer` y `runner` ven el shell, el dashboard
  y el trace, pero no los pasos de gestión de miembros (`owner`) ni los
  de edición y publicación de credenciales o puentes (`editor` y
  superiores).
- **Se adapta a la pantalla**: cada paso apunta a un elemento de la
  página en la que estás; si ese elemento no existe (otra ruta, una
  columna oculta en móvil), el paso se omite en lugar de romper el tour.
  El tour no navega entre páginas: para ver el paso del trace, abre una
  ejecución y relánzalo.
- **No habla con el servidor**: no hay endpoint ni tabla detrás; el tour
  entero viaja empaquetado con la consola y funciona sin internet. Navegar
  o cerrar sesión lo cierra y limpia sin dejar nada colgado.
- **Con teclado y con movimiento reducido**: `→`/`←` avanzan y
  retroceden, `Esc` cierra, los controles muestran foco visible, y si tu
  sistema pide movimiento reducido el tour no anima los cambios de paso.

## Las cinco páginas

| Página | Qué encuentras |
|---|---|
| **Ejecuciones** | El dashboard: métricas de salud (total, estados, duración media, tendencia diaria 7/30/90 días, desglose por puente), listado con filtros (puente, estado, fecha), auto-refresh opcional (5 s, pausado si la pestaña está oculta) y el **detalle con trace** paso a paso. |
| **Puentes** | Lista (con exportar/duplicar/importar), y el **editor**: cadena de bloques, formularios generados por los metadatos `ui`, guardado de borrador, validar, publicar, dry-run y el panel de versiones con diff y rollback. |
| **Bloques** | Biblioteca: filtros por texto/tipo/categoría, detalle de versiones con schemas y metadatos `ui`, creación y edición de `custom_logic` con el editor de código (errores de transpile con línea). |
| **Credenciales** | La bóveda write-only: crear, rotar e historial de versiones (ids y fechas, **jamás valores**). Eliminar exige teclear el nombre. |
| **Miembros** | Quiénes pertenecen al workspace, con rol y fecha de alta. Los `owner` añaden (por email de cuenta existente), cambian roles y expulsan; el resto ve la lista en lectura. |

## Leer una ejecución (el trace)

El detalle de una ejecución muestra cada paso en su posición de la cadena:

- **estado del paso**: `succeeded`, `failed`, `skipped` (una condición
  `when` dio falso: el paso NO se ejecutó, no es un error) o el estado
  global `running`/`pending`.
- **input y output de cada paso**, ya enmascarados si el puente define
  reglas `mask` (lo que ves es lo que se persistió: el enmascaramiento es
  irreversible).
- **el punto exacto del fallo**: paso, error y `code`
  (`REF_UNRESOLVED`, `TIMEOUT`, `BLOCK_FAILED`, `VALIDATION_FAILED` —
  ver [`errores-comunes.md`](errores-comunes.md)).
- **quién disparó** la ejecución (usuario con email, API key o `system`
  por cron/evento).
- Si la ejecución es un **replay**, enlaza al original; el original lista
  sus replays.

## Flujo típico: de idea a versión publicada

1. **Bloques**: si necesitas una pieza a medida, créala en *Bloques*
   (código TS + schemas + `ui`) y publica su versión 1.
2. **Puente**: crea el puente, añade instancias desde la biblioteca
   (los formularios salen de los metadatos `ui`), conecta los datos con
   referencias `$.` (guía:
   [`guia-bloques.md`](guia-bloques.md) y [`guia-puentes.md`](guia-puentes.md)).
3. **Ensayar (dry-run)**: ejecuta el borrador con datos de prueba, sin
   filas, sin webhook, sin eventos — con `mocks` por instancia si
   quieres fijar salidas.
4. **Validar**: los errores del validador aparecen sobre el canvas/editor;
   nada se publica con errores.
5. **Publicar versión**: se congela el snapshot exacto. Las ejecuciones
   en vuelo terminan en su versión; editar el borrador no las toca.
6. **Comparar y rollback**: en *versiones*, compara lado a lado
   (borrador↔versión, versión↔versión); el rollback re-publica una
   definición anterior como versión nueva — el pasado jamás se reescribe.

## Checklist semanal sugerido

- [ ] Métricas: ¿la tasa de éxito del rango bajó? ¿algún puente destaca en el desglose?
- [ ] Ejecuciones `failed` recientes: ¿el trace señala un bloque concreto?
- [ ] Credenciales: ¿alguna lleva demasiado sin rotar? (el historial lo muestra sin valores)
- [ ] Miembros: ¿alguien salió del equipo? (expulsar revoca su acceso al instante)

## El sistema visual (MVP6)

La consola usa el sistema **El rastro**: papel frío + tinta petróleo, donde el color solo significa estado (verde/`succeeded`, ladrillo/`failed`, ámbar/`pending`·`skipped`, azul/`running`) y la tipografía mono solo porta valores de máquina (slugs, triggers, JSON, duraciones). La firma está en Ejecuciones: cada fila dibuja su **rastro en miniatura** — un segmento por paso, proporcional a su duración real, coloreado por estado — para ver qué bloque es lento o falló sin abrir el detalle.

Los tokens viven en `console/src/tema.css` (`@theme` de Tailwind v4) y los componentes compartidos en `console/src/ui/` (con barrel en `ui/index.ts`) (`Boton`, `Insignia`, `Panel`, `Campo`, `Control`, `Esqueleto`). Para extender o ajustar: cambia el token en `tema.css` (el test de contraste AA `console/src/tema.test.ts` valida cada par y falla si algo baja de 4.5:1) y usa los componentes (`src/ui/`), las páginas viven en `src/paginas/` y la lógica pura en `src/lib/`; las páginas no inventan estilos. Las fuentes (Schibsted Grotesk + Spline Sans Mono) van autoalojadas en `src/fonts/` — la consola funciona sin internet.

## Ver también

- [`guia-bloques.md`](guia-bloques.md) — crear y publicar bloques paso a paso
- [`guia-puentes.md`](guia-puentes.md) — crear y publicar puentes paso a paso
- [`errores-comunes.md`](errores-comunes.md) — diagnóstico rápido de errores
