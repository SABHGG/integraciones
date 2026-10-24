# Proposal

## Why

El editor de puentes de la consola obliga hoy a escribir JSON a mano para el disparador (`{type, method, path}` / `{type, schedule, timezone}` / `{type, topic}`), para el `inputSchema` y para el `input` de cada instancia; las refs `$.` se teclean de memoria y un error tipográfico solo aparece al validar o, peor, al ejecutar. Construir un puente exige conocer de memoria el contrato del servidor; el objetivo del MVP de consola era lo contrario: que la consola sea la forma fácil de construir puentes y bloques.

## What Changes

- **Trigger con formulario guiado**: el disparador se edita con inputs reales — selector de tipo (http/cron/event/manual) y, según el tipo, verbo HTTP como dropdown (5 verbos), path, schedule cron de 5 campos con validación en cliente, timezone con lista IANA, y topic — sin renunciar a una pestaña JSON avanzada para casos excepcionales.
- **Constructor visual de inputSchema**: la textarea JSON cruda se sustituye por un editor de campos (nombre, tipo, requerido) que genera el JSON Schema, con pestaña JSON avanzada.
- **Autocompletado de refs `$.`**: los campos de config e input sugieren rutas disponibles (`$.trigger…` según el tipo de disparador, `$.now`, y las salidas de bloques previos expandidas desde su `output_schema`); en strings de config inserta interpolación `{$.ref}` y en input, mapeo directo `$.ref`.
- **Widgets `ui` completos**: `FormularioInstancia` pasa de pintar 5 widgets a soportar el contrato completo de `crates/api/src/ui.rs`: `select`, `json` y `ref-template` (además de text/url/number/textarea/password); el resto sigue cayendo a texto.
- **Autoría de metadatos `ui` desde la consola**: hoy ningún bloque de la biblioteca puede declarar `ui` (la consola publica `{code}` y el servidor guarda `ui = {}`), así que los formularios generados casi nunca materializan. El flujo de publicación de versión de bloque gana un editor de metadatos `ui` (title, order, widget, help).

Sin cambios de backend: `validate_trigger`, el contrato `ui`, la validación de cron/timezone/topic y los endpoints ya existen y validan en servidor; la consola deja de duplicar JSON y se apoya en el contrato vigente.

## Capabilities

### New Capabilities

(ninguna)

### Modified Capabilities

- `web-console`: formularios guiados del disparador; constructor visual de `inputSchema`; autocompletado de refs `$.` en config/input; soporte completo de widgets del contrato `ui` en formularios generados; autoría de metadatos `ui` al publicar versión de bloque.

## Impact

- **Consola** (`console/src/`): `paginas/bridges.tsx` (trigger, inputSchema, autocomplete en instancias), `ui/blockform.tsx` + `lib/ui-forms.ts` (widgets select/json/ref-template), `paginas/blocks.tsx` (autoría de `ui` al publicar), nuevos módulos en `lib/` (expansión de rutas `$.` desde `output_schema`, constructor de JSON Schema, catálogo IANA/validación cron ligera en cliente).
- **Sin cambios** en crates/api, crates/core, engine, sandbox ni migraciones: el contrato del servidor es la fuente de verdad y ya valida todo lo que el formulario guía.
- **Tests**: vitest (consola) para expansión de refs, generación de schema desde campos, widgets y validación cron/timezone en cliente.
