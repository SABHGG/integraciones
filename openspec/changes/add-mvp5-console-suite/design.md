# Design: add-mvp5-console-suite

## Context

La consola del MVP3 (Preact + Vite + CodeMirror, embebida, 180 KB gzip) cubre login, dashboard de ejecuciones con trace y editor de puentes. La API ya ofrece credenciales versionadas, biblioteca de bloques con `ui`, publicación y validación; faltan dos piezas de lectura (métricas agregadas, definición de una versión) y toda la superficie visual para operarlas. El MVP4 está **archivado**: la consola entra con email+contraseña (o API key de automatización), el `whoami` expone el rol y las páginas existentes ya lo respetan — este change hereda esa sesión y esa matriz, no añade auth.

## Goals / Non-Goals

**Goals:**
- Que todo lo operable por API sea visible y operable desde `/console`: credenciales, bloques, versiones, métricas.
- Dos endpoints de solo lectura que la consola necesita (stats, definición de versión) — nada de contratos nuevos de escritura.
- Export/import/duplicar como orquestación en cliente sobre endpoints existentes.

**Non-Goals:**
- Editor visual de triggers cron/eventos (siguen como JSON con ayuda contextual).
- Notificaciones/alertas, temas claro/oscuro, atajos de teclado.
- Backend de agregación con cache/materialized views (consulta directa índice-scaneada es suficiente a esta escala).

## Decisions

### D1 — Stats como agregación SQL directa
`GET /api/v1/stats{?from,to}` hace UNA consulta agregada sobre `executions` (filtro workspace + rango de `created_at`): serie diaria con `date_trunc('day', created_at)` (solo días con ejecuciones; la UI rellena los huecos del rango), conteos por estado y por puente (join a `bridges` para el slug), y duración media solo sobre exitosas (`avg(finished_at - started_at)`; ejecuciones sin `started_at` — `pending` — no se agregan). Límite 90 días de rango para acotar coste. Sin cache ni materialized views en v1: se documenta y se mide.

### D2 — Definición de versión congelada
`GET /bridges/{id}/versions/{version}` parsea el número (400 si no es entero, 404 si no existe), devuelve `{id, version, createdAt, definition}` tal cual el snapshot. El listado existente sigue liviano (sin definition): la consola la pide perezosamente para el diff/rollback.

### D3 — Diff estructural en cliente, sin librería
Diff JSON por rutas planas (`trigger.method`, `blocks.1.config.url`…) implementado en consola (~80 líneas) con tests de Vitest: descendente por primera diferencia, valores lado a lado, arrays por índice. Sin dependencia nueva y con una sola fuente de verdad (el JSON congelado del servidor).

### D4 — Rollback = re-publicación orquestada en cliente
Rollback: `GET` definición de la versión destino → `PATCH` del borrador sección por sección → `POST /versions`. El backend no sabe que es un rollback; la UI etiqueta la versión nueva como «rollback de la N». La inmutabilidad vigente (versiones congeladas, ejecuciones intactas) no cambia.

### D5 — Export/import: JSON de definición, sin identidad
Export = `{name, slug, definition}` con la definición (borrador o versión congelada vía D2). Import = `POST /bridges` → `PATCH` del borrador → `POST /validate` (errores en línea) → publicación explícita del usuario. Duplicar = import del export interno con slug/nombre sufijados. Nada de ids de workspace en el artefacto.

### D6 — Auto-refresh con visibility awareness
Polling del listado cada 5 s (apagado por defecto, selector por interacción), pausado cuando `document.hidden` y una sola petición en vuelo. El indicador muestra la última actualización. Sin websockets/SSE en v1.

### D7 — Páginas nuevas con el mismo conjunto de patrones
Credenciales y bloques replican el harness visual del editor (cards, errores del servidor en línea, `422` mapeado). El editor de código de `custom_logic` reutiliza `EditorCodigo`; los formularios de bloques reutilizan el generador `ui`.

### D8 — Superficies nuevas bajo la matriz de roles (MVP4)
Cada página nueva consulta el rol del `whoami` y usa el espejo `puede()` de `roles.ts` para ocultar lo que el rol no permite: stats y definición de versión son `GET` → legibles por `viewer`; las mutaciones (rotar/eliminar credencial, crear/publicar bloque, rollback, importar/duplicar) quedan visibles solo con `editor`. El servidor sigue siendo la única fuente de verdad: un `403` se muestra donde ocurre y ninguna página duplica la matriz — la espejo es solo presentación.

## Risks / Trade-offs

- **Stats sin cache**: en workspaces con millones de ejecuciones la agregación se degrada → límite de 90 días y documentado; escape futuro: tabla resumen daily.
- **Diff en cliente por rutas planas**: no highlight sintáctico dentro de strings; suficiente para revisar versiones a esta escala.
- **Rollback en cliente**: una carrera entre PATCH y publish mostraría 422 — flujo secuencial explícito en la UI.

## Migration Plan

Sin migraciones. Deploy: binario nuevo (dos endpoints de lectura nuevos); los assets de consola se reconstruyen (`npm run build`) antes de empaquetar en release — el CI ya lo hace.

## Open Questions

- Ninguna abierta bloqueante. La expiración de sesión de usuarios (MVP4) no se toca aquí.
