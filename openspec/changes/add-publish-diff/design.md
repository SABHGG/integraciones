# Design

## Context

La maquinaria de diff existe y está probada: `diffEstructural`/`resumenDiff`/`AUSENTE` (console/src/lib/diff.ts, aplanado a rutas de hoja, sin dependencias). `PanelVersiones` ya pide la definición congelada con `GET /bridges/{id}/versions/{n} → { definition }` y renderiza las líneas con su propio markup. El confirm de publicación (bridges.tsx, change add-editor-guided-forms) hoy solo muestra un resumen orgánico. La definición congelada (`BridgeDefinition`) es camelCase con las mismas secciones que el PATCH del borrador: `trigger`, `inputSchema`, `blocks` (instancias resueltas), `mask`, `emit`, `webhook`.

## Goals / Non-Goals

**Goals:** diff exacto del contenido que va a congelarse vs la última versión publicada, en el punto de máximo compromiso; reuso total de lib/diff.ts; cero duplicación del render.

**Non-Goals:** no se cambia backend ni se añade endpoint; no se difunde el diff por correo/emit; no se diff contra versiones arbitrarias (solo la última publicada); no se añade diff a la lista de puentes.

## Decisions

**D1. Lado izquierdo = última versión publicada, vía el endpoint ya existente.** `pedirPublicacion` tras validar: si `puente.latestVersion` existe → `GET /bridges/{id}/versions/{latestVersion}` → `{ definition }`. El `latestVersion` ya está en el `GET /bridges/{id}` que recarga el editor. *Alternativa descartada:* nuevo endpoint `?diff=previous` (backend sin necesidad) o exportar el artefacto (formato distinto).

**D2. Filtro de rutas como función pura en lib/diff.ts.** Nueva `lineasDeSecciones(lineas, secciones)`: conserva solo rutas cuya primera parte esté en las secciones editables (`trigger`, `inputSchema`, `blocks`, `mask`, `emit`, `webhook`) y descarta siempre las rutas `blocks.*.code` — el código vive en la versión del bloque (el cambio de bloque ya es visible en `blockVersionId`); mostrar el JS resuelto como «diferencia del puente» sería ruido de resolución. *Alternativa descartada:* reconstruir en cliente la definición normalizada del borrador (duplicaría `definicion_del_borrador` del servidor, con drift garantizado).

**D3. Render compartido extraído a `ui/tabla-diff.tsx`.** El markup de líneas de `PanelVersiones` (ruta + valor izq/der, `AUSENTE` atenuado, mono para rutas y valores de máquina, etiquetas de lado) se extrae a un componente con props (`lineas`, `etiquetaIzq`, `etiquetaDer`, `plegable`), y `PanelVersiones` lo adopta. En el confirm va dentro de un `<details>` con `max-h` acotado: la confirmación no puede desplazar la página ni ocultar el botón irreversible. Reglas de El rastro: rutas y valores en mono, etiquetas de lado en grotesca, sin croma de estado (el diff no es un error: es información).

**D4. Primera versión sin diff.** `puente.latestVersion === null` → la confirmación muestra «primera versión — no hay una anterior con la que comparar» junto al resumen orgánico; sin petición del diff.

**D5. Fallo de carga = degradación honesta.** Si el GET de la definición anterior falla, `pedirPublicacion` no bloquea: la confirmación muestra el resumen orgánico y una nota «no se pudo comparar con v{n}» (el usuario decide con lo que hay; el servidor valida de nuevo al congelar).

**D6. Estado de confirmación enriquecido.** `confirmacion` pasa de string a `{ resumen: string; primeraVersion?: true; sinDiff?: string; lineas?: LineaDiff[]; etiquetas?: [string, string] }`; el panel pinta resumen orgánico + diff cuando hay líneas. El botón «Publicar ahora» (variante `peligro`, R2) queda intacto.

## Risks / Trade-offs

- [La definición congelada normaliza campos que el borrador ni siquiera declara] → D2 filtra secciones y `blocks.*.code`; si aparece otra ruta de resolución, es una línea más de información (no bloquea).
- [Un GET extra por confirmación] → solo cuando hay versión previa; es el mismo GET que ya hace PanelVersiones.
- [Diff largo en cadenas grandes] → `<details>` plegable con scroll interno; el resumen (`resumenDiff`) está siempre arriba.
- [Race: el borrador cambia entre validar y publicar] → la confirmación usa el `draft` del render actual; el servidor revalida al congelar (la última palabra sigue siendo suya).

## Migration Plan

Sin migraciones: front-end de consola servido por el binario. Rollback = revertir el commit; ningún dato persistido cambia.

## Open Questions

(ninguna que bloquee implementación.)
