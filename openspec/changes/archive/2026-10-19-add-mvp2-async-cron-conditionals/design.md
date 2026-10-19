# Design: add-mvp2-async-cron-conditionals

## Context

MVP1 dejó el ejecutor desacoplado del transporte (el disparo solo encola en `executions`, la tabla ES la cola con SKIP LOCKED) y el schema ya nació preparado: `executions.webhook_url` (MVP2), `webhook_deliveries` y `missed_triggers` existen vacías, y `trigger_type` admite `cron|event|manual` aunque solo `http` está implementado. `execution_steps.status` admite únicamente `succeeded|failed` — hay migración. El plano `/x` no lleva auth de administración y tiene middlewares propios (rate-limit), reutilizables para polling.

## Goals / Non-Goals

**Goals:**
- Async 202 + polling sin tocar el contrato síncrono existente.
- Webhook de resultado firmado y con reintentos persistentes.
- Scheduler cron dentro del binario actual, sin nueva infraestructura (sin Redis, sin cron externo).
- Enrutado condicional (`when`/`switch`) validado en diseño y registrado como `skipped` en el trace.

**Non-Goals:**
- Eventos/colas, dry-run, replay, rotación de bóveda, enmascaramiento, UI (MVP3).
- Auth por puente en `/x` (queda para versión futura, como en MVP1).
- Política de solapamiento configurable (fija `skip` en MVP2) ni política de reintentos configurable por puente.
- Múltiples triggers por puente (sigue siendo un trigger único).

## Decisions

### D1 — Async opt-in por petición con `Prefer: respond-async`
El llamador elige el modo por request (header estándar RFC 7240) en vez de un flag por puente: el mismo snapshot publicado sirve ambos modos y el modo síncrono queda como default intacto. Alternativa descartada: `?async=true` en la query (contamina la URL y choca con firmas/cacheo). `202` lleva `Location: /x/{slug}/executions/{executionId}`. `Idempotency-Key` funciona igual: la repetida devuelve la ejecución original (único parcial existente) con su `Location`.

### D2 — Polling acotado al slug, sin auth
`GET /x/{slug}/executions/{executionId}` responde estado (`pending|running`) o el sobre final idéntico al síncrono. El 404 cross-slug evita enumerar resultados de otros puentes. Riesgo asumido y documentado: el recurso no lleva auth (mismo plano que el disparo); el executionId es UUID v7 no adivinable y los datos expuestos son exactamente los que un caller del puente ya recibe. Para flujos sensibles se recomienda webhook (D3).

### D3 — Webhook por puente, secreto generado por el sistema
`webhook: {url}` (https obligatorio) vive en la definición y se congela en el snapshot; al encolar, la URL se copia a `executions.webhook_url` (semántica de snapshot: la versión que corrió es la que entrega). Secreto HMAC de 256 bits generado por puente, expuesto solo por el plano admin. Firma HMAC-SHA256 sobre el body crudo + `X-Integraciones-Timestamp`; entrega deduplicable por `X-Integraciones-Delivery` (id de entrega estable, no por intento).

### D4 — Entregas como segunda cola sobre su propia tabla
Mismo patrón que `executions`: job en el binario api que consume `webhook_deliveries` con `status='pending' AND next_retry_at <= now()` vía `FOR UPDATE SKIP LOCKED`. Backoff fijo 30 s → 2 m → 10 m → 1 h → 6 h (5 intentos), éxito = 2xx. Alternativa descartada: reintentar en memoria (se pierden al reiniciar) o librería externa de outbox (nueva dependencia sin necesidad).

### D5 — Scheduler cron dentro del binario api (instancia única en MVP2)
Job tokio que cada segundo: (1) recalcula la próxima ocurrencia de cada puente con última versión publicada de trigger cron (join a `bridge_versions` sobre `snapshot->>'trigger'`) y pre-encola la fila `pending` con `run_at` = instante programado — el índice parcial de la cola ya ordena por `run_at`, el worker la reclama cuando vence; (2) al encolar comprueba solape: si la ejecución anterior sigue `pending|running`, registra `missed_triggers(overlap)` y no encola; (3) al arrancar, para cada puente cron calcula las ocurrencias vencidas desde la última programada (derivada de `executions.trigger_data->>'scheduled_at'`, sin tabla de estado nueva): encola solo la más reciente y registra el resto como `downtime`. Limitación documentada: con varias instancias del api habría que elegir líder — fuera de alcance MVP2. Deps nuevas: parser cron de 5 campos + `chrono-tz`.

### D6 — `switch` como bloque built-in ejecutado por el host
Mismo patrón que `http-get`/`http-post`: bloque built-in `switch` sin código, cuya config define `routes: [{when, to}]` + `default` (ids de instancia posteriores). `when` es una propiedad opcional de cualquier instancia. El validador de publicación comprueba: `to` apunta a instancia existente con índice mayor (sin ciclos, estructuralmente imposible retroceder), condiciones conformes a la gramática declarativa, y rechaza JS arbitrario. Gramática de condiciones en `crates/domain`: objeto de un operador (`eq/ne/gt/gte/lt/lte/exists/contains`) con combinadores `and`/`or`/`not`, evaluada en Rust sobre el contexto — el sandbox no participa en decisiones de ruta.

### D7 — Pasos `skipped` con migración de CHECK
Migración `0002`: sustituye el CHECK de `execution_steps.status` por `succeeded|failed|skipped` y añade índice parcial para la cola de entregas (`webhook_deliveries` pending por `next_retry_at`). Los pasos `skipped` se registran en su `seq` original sin input/output/duración, en la misma transacción que el resto del trace.

## Risks / Trade-offs

- **Polling sin auth** expone el resultado a quien tenga el executionId → mitigado con UUID v7 + acotación por slug; webhook recomendado para datos sensibles.
- **Scheduler monoproceso**: si el api se despliega con réplicas, habría dobles encolados → MVP2 asume instancia única (documentado); la cola SKIP LOCKED ya protege al worker.
- **Backoff fijo** de webhook no cubre ventanas largas de caída del receptor → 5 intentos/8 h es suficiente para MVP2; política configurable queda para más adelante.
- **Pre-encolar una sola ocurrencia** simplifica el solape, pero un puente de frecuencia menor a 1 s saturaría el scheduler → límite práctico: schedules ≥ 1 minuto.
