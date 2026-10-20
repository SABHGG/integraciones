# Proposal

## Why

La biblioteca de bloques crece sin techo: cada bloque nuevo alarga la lista y, con el detalle debajo de la tabla, inspeccionar el bloque 50 exige cazar el panel con scroll. El patrón ya resuelto en puentes (lista → ruta propia `/bridges/{id}`) encaja aquí: el detalle de bloque se vuelve página propia con URL, filtros de lista conservados en la query y editor a ancho completo.

## What Changes

- **Ruta `/console/blocks/{id}`**: versiones, schemas y edición de `custom_logic` (editor de código + publicación) viven en la página del bloque, con botón de vuelta a la biblioteca.
- **Lista con filtros en la URL**: texto, tipo y categoría de la biblioteca se guardan en la query (`/console/blocks?q=…&tipo=…`); al volver del detalle la lista conserva exactamente ese estado.
- Sin cambios de backend: consume los endpoints existentes (`GET /blocks/{id}`, `GET /blocks/{id}/versions`, `POST /blocks/{id}/versions`).

## Capabilities

### Modified Capabilities

- `web-console`: el detalle de bloque pasa a ruta propia navegable y compartible; la lista conserva sus filtros en la URL. Roles vigentes sin cambios (lectura = viewer, edición = editor).

## Impact

- **Consola**: `main.tsx` (una ruta nueva), `paginas/blocks.tsx` (división lista/detalle). Sin migraciones ni endpoints nuevos.
- **Compatibilidad**: los enlaces a `/console/blocks` siguen funcionando; el detalle anterior (panel embebido) desaparece a favor de la ruta.
