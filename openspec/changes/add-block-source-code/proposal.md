# Proposal

## Why

Al publicar una versión de un bloque `custom_logic`, el plano transpila el TypeScript con swc y `block_versions.code` guarda **solo** el JavaScript resultante: el fuente original se pierde para siempre. Eso deja dos agujeros visibles hoy: una versión inmutable no se puede auditar ni evolucionar desde su fuente (solo queda el JS emitido), y la página de detalle del bloque no muestra el código de ninguna versión, así que alguien nuevo no puede saber qué hace un bloque sin ir a `curl` (y el editor de "publicar nueva versión" arranca siempre de una plantilla fija).

## What Changes

- **El fuente original no se pierde**: la publicación de un `custom_logic` guarda el TypeScript enviado junto al JS transpilado, en una columna nueva `block_versions.source_code` (migración `0008`). La versión inmutable pasa a conservar ambos artefactos.
- **La API expone ambos**: `GET /blocks/{id}/versions` devuelve `sourceCode` (fuente original, `null` cuando no existe) junto a `code`, que **mantiene su semántica** (el JS que ejecuta el sandbox).
- **El detalle del bloque muestra el código**: visor por versión en `/console/blocks/{id}` — fuente original cuando existe; si no, el JS con etiqueta honesta de lo que es; bloques built-in sin código se señalan como tales.
- **El editor arranca del código real**: "publicar nueva versión" se precarga con el `sourceCode` de la última versión (plantilla actual solo cuando no hay fuente).
- `crates/api/openapi.json` actualizado con el campo nuevo; `docs/guia-bloques.md` explica que el fuente se conserva y dónde se ve.

**Fuera de alcance** (decidido, no se hace aquí):

- Recuperar los fuentes de versiones históricas: es imposible, nunca se guardaron. No hay backfill ni reconstrucción.
- Type-checking del TypeScript: el transpile sigue siendo solo emisión de JS; no se añade comprobación de tipos.
- Cambiar el contrato del runtime: el sandbox sigue viendo y ejecutando únicamente `code`.
- Mostrar código en bloques built-in `http`: no tienen código propio.

## Capabilities

### Modified Capabilities

- `block-library`: la publicación de un bloque `custom_logic` preserva el fuente original junto al JS transpilado y la versión expone ambos artefactos.
- `web-console`: el detalle del bloque muestra el código de cada versión y el editor de nueva versión se precarga con el fuente de la última.

## Impact

- **Backend**: migración `crates/api/migrations/0008_block_source_code.sql`, `crates/api/src/blocks.rs` (publicación y DTO), `crates/api/openapi.json`, tests en `crates/api/tests/`.
- **Consola**: `console/src/paginas/blocks.tsx` (tipo `VersionBloque`, visor por versión, precarga del editor), posible helper puro en `console/src/lib/` con su test de vitest.
- **Datos**: las filas de `block_versions` de `custom_logic` crecen (fuente + JS) — aceptado; las versiones publicadas antes del change quedan con `source_code` NULL.
- **Compatibilidad**: aditivo. Los consumidores actuales de `code` y el ejecutor no cambian; ningún endpoint cambia de forma.
