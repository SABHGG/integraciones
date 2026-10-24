# Diagramas interactivos

Capa visual de documentación generada con [Archify](https://github.com/tt-a1i/archify) a partir de las especificaciones de `openspec/specs/` y del código real. Cada HTML es un artefacto autónomo (SVG embebido, sin dependencias): sirve en `/docs`, se abre en el navegador e incluye tema claro/oscuro, pan/zoom, búsqueda, vistas guiadas y trazado de relaciones.

> Los diagramas se autoran en español; la UI del visor (botones, tooltips) está fijada en inglés por el renderer.

## Los cinco diagramas

| Diagrama | Tipo | Qué cubre |
|---|---|---|
| [`arquitectura.html`](arquitectura.html) | `architecture` | Mapa de componentes: dos planos HTTP (`/api/v1` admin, `/x/{slug}` invocación), engine + worker, sandbox QuickJS, bóveda, Postgres como cola+registro, eventos. |
| [`invocacion-puente.html`](invocacion-puente.html) | `sequence` | Una llamada `POST /x/{slug}` de principio a fin: idempotencia, sync inline vs async 202, `SKIP LOCKED`, paso k en el sandbox, `ctx.http`, commit del trace, webhook firmado. |
| [`publicacion.html`](publicacion.html) | `workflow` | De borrador a versión inmutable: transpile swc al publicar, schemas estrictos (422 sin crear versión), dry-run con mocks, rollback por re-publicación. |
| [`ciclo-ejecucion.html`](ciclo-ejecucion.html) | `lifecycle` | Estados de `executions`: pending → running → succeeded/failed, fail-fast con códigos estables, replay con `replayOf`, purga por retención. |
| [`rastro-datos.html`](rastro-datos.html) | `dataflow` | Observabilidad: trigger_data por tipo de disparo, mask `***` antes de persistir, `executions`/`execution_steps`, El rastro en la consola, retención 30 d / 365 d. |

## Lectura sugerida

1. **Arquitectura** para el mapa general y los dos planos.
2. **Invocación** para el recorrido de una ejecución concreta.
3. **Ciclo de vida** para los estados y salidas (error, replay, purga).
4. **Publicación** para el camino del código hasta la versión congelada.
5. **Rastro de datos** para dónde duermen los datos y quién los lee.

## Fuentes y regeneración

Cada HTML viene de un JSON tipado en este mismo directorio (`*.architecture.json`, `*.sequence.json`, `*.workflow.json`, `*.lifecycle.json`, `*.dataflow.json`). Para regenerar tras editar un JSON:

```bash
node <skill-archify>/bin/archify.mjs deliver <tipo> docs/diagramas/<nombre>.<tipo>.json docs/diagramas/<nombre>.html --quality showcase
```

Tipos válidos: `architecture`, `sequence`, `workflow`, `lifecycle`, `dataflow`. La calidad `showcase` exige 9 verificaciones de artefacto sin errores ni avisos de composición.

## Evidencia de verificación

Junto a cada HTML quedan los recibos de la última verificación:

- `*.visual-check.json` — recibo de navegador: containment (sin scroll en 1440×900, 1600×1000, 1920×1080 y 2048×1320, claro y oscuro) y readability (texto proyectado ≥ 6 px), en `pass`.
- `*.visual-check.*.png` — capturas por viewport y tema.
- `*.visual-check.html` — hoja de contactos.

Los JSON fuente no deben editarse a mano sin re-validar: cualquier cambio de geometría (posiciones, `labelAt`, canales) puede romper las invariantes que exige el validador.
