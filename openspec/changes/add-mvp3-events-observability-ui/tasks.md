# Tasks: add-mvp3-events-observability-ui

## 1. Fase 1 — Bóveda versionada con rotación

- [x] 1.1 Migración: tabla `credential_versions` (valor cifrado, `rotated_at`), copia del secreto vigente como versión 1 y `credentials.current_version_id`; aplicable sobre el schema existente
- [x] 1.2 Escritura versionada: `POST`/`PATCH /api/v1/credentials` crea versión inmutable y mueve la vigente; historial de metadatos en `GET` (sin valores nunca); tests RED/GREEN
- [x] 1.3 `ctx.credential` resuelve la versión vigente; registro de `executions.credentials_used` (pares credential/version) en la transacción del trace; test contra Postgres real
- [x] 1.4 Auditoría: detalle de ejecución expone las versiones usadas, verificables contra el historial; test

## 2. Fase 2 — Enmascaramiento

- [x] 2.1 Validador de publicación: `mask: [paths]` con la gramática `$.` existente, congelado en snapshot; tests del validador
- [x] 2.2 Ejecutor: sustitución `***` sobre IO de pasos y sobre final antes de persistir, en la misma transacción del trace; test de irreversibilidad (no queda valor en ninguna tabla)
- [x] 2.3 Superficies: trace/detalle/listado y webhook entregan el dato enmascarado; editar reglas no reescribe ejecuciones de versiones previas; tests

## 3. Fase 3 — Eventos/colas

- [x] 3.1 Validador: trigger `{type: "event", topic}` con formato de topic, congelado en snapshot; suscripción múltiple al mismo topic; tests
- [x] 3.2 `POST /api/v1/events/{topic}`: fan-out transaccional (una ejecución por suscriptor publicado, `trigger_data` con payload + eventId), `202` sin suscriptores; test contra Postgres real
- [x] 3.3 `emit: {onSuccess, onFailure}` congelado en snapshot y publicado dentro de la transacción de finalización, con sobre final + `bridgeSlug` + `executionId`; tests de encadenado A→B y de emisión solo en fallo
- [x] 3.4 Documentar límites: sin orden ni entrega garantizada entre suscriptores (README)

## 4. Fase 4 — Dry-run y replay

- [x] 4.1 Motor en modo dry-run: ejecuta el borrador con las reglas del ejecutor (refs, schemas, fail-fast, timeout) sin persistir filas ni disparar webhook/emit; trace completo en la respuesta; tests
- [x] 4.2 `POST /api/v1/bridges/{id}/dry-run` con mocks por instancia (`mock: {output}` salta sandbox/HTTP y alimenta el contexto); tests de cadena mixta mock/real
- [x] 4.3 `POST /api/v1/executions/{id}/replay`: clona `bridge_version_id` + entrada, `trigger_data.replayOf`, corre por la cola normal; test de replay de una ejecución http
- [x] 4.4 Enlace bidireccional original ↔ replays en detalle y listado; tests

## 5. Fase 5 — Consola web

- [x] 5.1 Elegir framework SPA ligero (criterio del design D6) y andamiaje: build estático TypeScript embebido en el binario y servido bajo `/console` con el plano admin como única API; test de serving y auth
- [x] 5.2 Login con API key (Bearer en todas las llamadas, error del plano admin visible en key inválida); tests e2e del flujo
- [x] 5.3 Dashboard de ejecuciones: listado con filtros y cursor, detalle, visor de trace con pasos `skipped` e IO enmascarado; test con ejecución failed real
- [x] 5.4 Gestión de puentes: editor visual de la cadena (orden, `when`, rutas de `switch`) que muestra los 422 del validador del servidor sobre el canvas; test de ruta hacia atrás marcada
- [x] 5.5 Formularios generados desde el campo `ui` de la versión del bloque (+ JSON genérico si no hay `ui`); test con `http-get`
- [x] 5.6 Editor de código TS/JS para `custom_logic` con errores de transpile/schema en línea al publicar; test de error de sintaxis mostrado

## 6. Fase 6 — Cierre

- [x] 6.1 E2E punta a punta: puente A emite `onSuccess` → puente B suscrito se ejecuta; dry-run con mock; replay verificable; mask visible en trace y webhook
- [ ] 6.2 README: secciones de eventos, dry-run, replay, bóveda versionada, mask y consola (`/console`), con ejemplos
- [ ] 6.3 OpenAPI actualizado (eventos, dry-run, replay, historial de credenciales) y validado en Scalar; CI verde (`cargo test --workspace`, `clippy -D warnings`); commits por unidad de trabajo
