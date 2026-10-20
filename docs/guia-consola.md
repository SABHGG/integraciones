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
   [`guia-bloques-y-puentes.md`](guia-bloques-y-puentes.md)).
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

## Ver también

- [`guia-bloques-y-puentes.md`](guia-bloques-y-puentes.md) — crear bloques y puentes paso a paso
- [`errores-comunes.md`](errores-comunes.md) — diagnóstico rápido de errores
