# Design: add-mvp1-core

## Context

Proyecto greenfield. El diseño completo fue decidido en 9 rondas de requisitos antes de este change. Este documento consolida **todas** las decisiones para que ninguna se pierda. Las decisiones que no aplican a MVP1 quedan marcadas como (MVP2/MVP3) pero documentadas aquí porque son contrato futuro que condiciona lo que sí se construye.

## Goals / Non-Goals

- **Goals**: corte vertical MVP1 completo; validar sandbox y ejecutor contra APIs reales; dejar el esquema de BD definitivo desde el día 1.
- **Non-Goals**: UI, disparo async, cron, eventos, webhooks de resultado, dry-run con mocks, replay (ver Roadmap en project.md).

## Decisions

### Decision: Motor de sandbox — rquickjs + quickjs-ng
- Cold start ~µs por isolate → modelo "isolate fresco por ejecución" viable.
- Memoria ~1MB/isolate; `set_memory_limit` + interrupt handler nativos = timeout por puente en ms que corta bucles infinitos.
- Sin JIT → menor superficie de ataque; validado en producción (Svix 11ms→1ms, LLRT de Amazon).
- Carga IO-bound: el JIT de V8 no aporta; descartados deno_core/rusty_v8 (peso: 600k LOC C++), boa (experimental pre-1.0), WASM/wasmtime (doble indirección).
- Puerta abierta: si se necesita JIT futuro, migración natural a deno_core; el contrato ejecutor↔sandbox se diseña independiente del motor.

### Decision: Modelo de aislamiento y límites
- **Isolate fresco por ejecución** (crear → correr → destruir). Sin pool de calientes (ahorro marginal, complica limpieza de estado).
- Timeout **por puente en ms**; el interrupt del isolate corta incluso bucles infinitos. Detalle: llamadas HTTP salientes necesitan su propio deadline además del interrupt.
- Límites de payload: defaults globales configurables + override por puente; aplican a body HTTP de entrada y output de bloques; exceder = fail-fast. Sin truncado (mata la fidelidad del replay). Spill por referencia a blob storage = futuro.

### Decision: Convención de bloques CUSTOM_LOGIC
- Función única con export por defecto: `export default function(input, ctx) → salida | Promise`.
- Metadatos (nombre, icono, categoría, description, schemas, ui) viven en `BlockDefinition`; la config de instancia vive en el puente. El código solo transforma entrada en salida.
- Errores: `throw` nativo; el ejecutor captura y registra en el trace. Sin convención de resultado-error.
- `ctx` es el único acceso a capacidades del host: `ctx.http.request({url, method, headers, body})` (timeout obligatorio, allowlist de dominios opcional por workspace, respuesta texto o JSON parseado), `ctx.log` estructurado, `ctx.crypto` (hmac/sha/base64), helpers JSON, `ctx.credential(name)`.
- Sin npm en v1 (requeriría bundling + vetado de paquetes); la lista de host functions es la frontera de seguridad y crece incrementalmente.

### Decision: Pipeline TypeScript
- Transpilar con **swc al publicar** la versión; la versión inmutable guarda el JS resultante. El runtime nunca ve TS; errores de compilación se detectan al publicar.

### Decision: Modelo de datos entre bloques ("B puro")
- Cada bloque deja su salida nombrada en un **contexto acumulado**; cada bloque declara su entrada como mapeo de campos a referencias.
- Sintaxis: un string es referencia **si y solo si** empieza con `$.`; cualquier otro valor JSON es literal.
- Raíces reservadas: `$.trigger` (entrada del disparo), `$.now` (timestamp ISO de inicio). Arrays por índice numérico (`$.ordenes.0.total`); refs a nodo completo permitidas.
- Una gramática en dos anclajes: mapeo de entrada directo; interpolación en strings de config: `https://api.x.com/users/{$.auth.userId}`.
- Runtime: ref no resuelta = **fail-fast en resolución** con mensaje quirúrgico ("referencia $.auth.userId no resolvió en bloque X").
- Validación estática al publicar: cada ref se verifica contra el outputSchema del bloque origen; ruta verificable e inexistente = error de publicación; ruta no verificable (schema suelto) = se permite sin ruido.

### Decision: Contrato del trigger (por tipo de disparador)
| Disparador | `$.trigger` |
|---|---|
| HTTP | `{body, query}` — headers del llamador **jamás** entran al contexto ni al IO |
| Cron (MVP2) | `{scheduledAt, cronExpr, timezone}` — hora programada, no real |
| Evento (MVP3) | `{topic, payload, receivedAt}` |
| Manual/dry-run | datos de prueba conformes al inputSchema del puente |
- HTTP acepta solo `application/json` en v1; otro Content-Type → 415 Problem Details.
- inputSchema del puente (opcional): valida el body entrante (inválido → sobre 200 `VALIDATION_FAILED` sin ejecutar), genera formulario de datos de prueba, hace verificables las refs `$.trigger.…`.

### Decision: Semántica de errores y respuesta
- **Fail-fast simple**: el primer error termina la ejecución con el error registrado en el trace.
- Respuesta del plano invocación: sync = **200 con sobre único** `{success, data, error?, executionId}`; async = **202 con `{success, executionId}`** (MVP2). El 200-siempre rige el *resultado*, no el acuse de recibo.
- `error` con **códigos estables** (enum: `BLOCK_FAILED`, `VALIDATION_FAILED`, `TIMEOUT`, `NOT_FOUND`…), `message`, `block`, detalles. Contrato firme para consumidores.

### Decision: Ejecución y observabilidad
- Ejecutor **desacoplado del transporte HTTP** desde el día 1; graba su trace al store siempre.
- Store obligatorio en v1: cada ejecución con IO por bloque (input/output/duración/error) en `execution_steps`.
- Stateless entre ejecuciones: ninguna corrida depende de estado de otra.
- Replay (MVP2) posible por el snapshot de definición + IO persistido.

### Decision: API REST v1
- **Dos planos separados**: administración `/api/v1/*` e invocación `/x/{slug}` — superficies, middlewares y rate-limits aislados.
- Versionado por prefijo de ruta (`/v1`).
- Admin: errores **RFC 9457** (application/problem+json) con status HTTP reales.
- Invocación: fallos de transporte (404 slug, 405 método, 413 body, 415 content-type) = HTTP semántico + Problem Details; resultado de ejecución = sobre 200.
- Auth admin: `Authorization: Bearer` con API key global (hoy) → mismo header llevará JWT cuando llegue identidad real, sin romper clientes.
- IDs: **UUID v7** interno + **slug** legible de invocación por workspace.
- Paginación por **cursor opaco** en todos los listados.
- Trigger es parte del borrador del puente y se snapshot-a en cada versión; no es recurso separado.
- `Idempotency-Key` opcional en disparo HTTP: repetida → devuelve la ejecución previa.

### Decision: Modelo de datos Postgres
- Definición de puente como **documento JSONB** por versión (`bridge_versions.definition`): bloques instanciados + inputs + configs + trigger + input_schema, leído atómicamente por el ejecutor. Sin normalización de instancias.
- Tablas: `users`, `workspaces`, `workspace_members` (owner|editor|runner|viewer), `api_keys` (key_hash), `credentials` (secret_encrypted, write-only), `blocks`, `block_versions` (UNIQUE block_id+version), `bridges` (UNIQUE workspace_id+slug), `bridge_versions` (UNIQUE bridge_id+version), `executions`, `execution_steps`, `webhook_deliveries` (MVP2), `missed_triggers` (MVP2).
- `executions` **es la cola**: `status='pending'` + `run_at` + índice parcial + `FOR UPDATE SKIP LOCKED`. Ejecución y fila nacen atómicas. Redis solo si el volumen lo exige (cola detrás de abstracción).
- Índices críticos: cola parcial; cursor `(bridge_id, created_at DESC)`; unique parcial `idempotency_key`; `(execution_id, seq)`.
- Retención: IO (`execution_steps`) 30 días; resumen 1 año; configurables; job de purga por `created_at`.
- Tablas de workspace/identidad existen desde v1 aunque la auth sea una sola key: evitar backfill multi-tenant después.

### Decision: Secretos y datos sensibles
- Credenciales **compartidas por referencia** (pool por workspace, no duplicadas por usuario); el host resuelve el valor al momento de la llamada y nunca entra al payload entre bloques ni al IO persistido.
- Enmascaramiento de claves sensibles como red de seguridad + retención ~30 días (enmascaramiento completo en MVP3).
- Headers del llamador HTTP excluidos del contexto (fugas de tokens de consumidores).

### Decision: Generación de UI futura (compromiso de diseño)
- Convención de metadatos de presentación (`ui`: title, orden, widget, help) junto a cada schema desde el día 1, y formato especial para strings con interpolación de refs (`ref-template`).
- JSON Schema restringido a **subconjunto estricto documentado** (types básicos, properties, required, enum, items, format) — determinismo para validador y generador de formularios.
- Formularios generables: config de instancia (configSchema), mapeo de entrada (inputSchema + autocompletado de refs), datos de prueba del dry-run.

## Risks / Trade-offs

- **rquickjs es la ruta crítica**: se valida primero (Fase 1) con suite de bloques trampa antes de construir el resto.
- Intérprete sin JIT: ~10-30x más lento que V8 en CPU pura — aceptado (carga IO-bound).
- Ni QuickJS ni V8 son boundary de seguridad del OS: para código verdaderamente hostil el escape es proceso separado (el diseño stateless lo permite a futuro).
- Interrupt no cancela sockets HTTP en curso: `ctx.http.request` necesita deadline propio además del timeout del puente.
- Documento JSONB: menos consultable campo a campo que un modelo normalizado — aceptado porque el ejecutor lo consume como unidad atómica.

## Open Questions

- Nombre del proyecto (provisional: "Bloques y Puentes").
- Elección final de crates al construir (candidatos: axum, sqlx, swc, rquickjs).
- Auth del plano de invocación: sin auth en v1; mecanismo por puente a futuro.
