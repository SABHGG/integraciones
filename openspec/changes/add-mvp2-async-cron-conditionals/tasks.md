# Tasks: add-mvp2-async-cron-conditionals

## 1. Fase 1 — Dominio: condiciones y esquema

- [x] 1.1 Gramática de condiciones en `crates/domain`: evaluador declarativo (`eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `exists`, `contains` + `and`/`or`/`not`) sobre refs `$.`, con tests unitarios de cada operador y combinador (RED/GREEN)
- [x] 1.2 `REF_UNRESOLVED` al evaluar una condición con referencia no resuelta (mismo criterio que los mapeos de entrada), con test
- [x] 1.3 Migración `0002`: CHECK de `execution_steps.status` pasa a admitir `skipped` + índice parcial de `webhook_deliveries` (pending por `next_retry_at`); migración aplicable desde 0001 existente
- [x] 1.4 Parser de cron de 5 campos + timezone IANA (`chrono-tz`) en el workspace: validación y cálculo de próximas ocurrencias, con tests de horarios de verano

## 2. Fase 2 — Enrutado condicional (engine + validación)

- [x] 2.1 Ejecutor: instancia con `when` falso → paso `skipped` sin invocar sandbox, cadena continúa; tests de trace con `skipped`
- [x] 2.2 Ejecutor: bloque built-in `switch` — evalúa rutas en orden, salta a la primera verdadera (o `default`), rutas no tomadas quedan `skipped`, sin evaluar condiciones restantes; tests por ruta
- [x] 2.3 Fail-fast preservado en la ruta tomada: error de bloque o de schema corta la ejecución aunque haya `when`/`switch`; tests de regresión MVP1
- [x] 2.4 Validador de publicación: `to` de cada ruta apunta a instancia existente y posterior (rechaza salto hacia atrás con 422), `when` conforme a la gramática (rechaza JS arbitrario), trigger cron válido; tests del validador
- [x] 2.5 Persistencia: pasos `skipped` grabados en su `seq` original sin IO/duración, misma transacción del trace; test contra Postgres real

## 3. Fase 3 — Disparo asíncrono y polling

- [x] 3.1 `POST /x/{slug}` con `Prefer: respond-async`: encola y responde `202` + `{success, executionId}` + `Location`; errores de transporte (404/405/413/415) intactos antes de encolar; tests
- [x] 3.2 Idempotencia en async: repetición con misma `Idempotency-Key` devuelve ejecución original y su `Location` sin nueva fila; test
- [x] 3.3 `GET /x/{slug}/executions/{executionId}`: estado `pending|running` sin `data`, sobre final idéntico al síncrono al terminar, 404 cross-slug; tests (con rates-limit del plano /x verificado)
- [x] 3.4 OpenAPI (`crates/api/openapi.json`) actualizado con 202, `Location` y el endpoint de polling; validar con la UI de Scalar

## 4. Fase 4 — Webhook de resultado firmado

- [ ] 4.1 Definición: `webhook: {url}` validada (https obligatorio) al publicar y congelada en snapshot; secreto HMAC por puente generado y expuesto solo por el plano admin; tests del validador y del endpoint
- [ ] 4.2 Al terminar una ejecución de puente con webhook: crear fila `webhook_deliveries` con la URL copiada a `executions.webhook_url`, payload = sobre final + metadatos; test contra Postgres real
- [ ] 4.3 Job de entregas: consume pending con `SKIP LOCKED`, firma HMAC-SHA256 del body crudo + `X-Integraciones-Timestamp` + `X-Integraciones-Delivery`; servidor local de pruebas verifica firma; tests
- [ ] 4.4 Reintentos: backoff 30 s/2 m/10 m/1 h/6 h, 5 intentos, éxito solo 2xx, estado final `delivered|failed` con último `response_status`; tests con servidor que falla N veces
- [ ] 4.5 Entrega también en fallo de ejecución (`success: false` + `error.code`); test

## 5. Fase 5 — Scheduler cron y disparo manual

- [ ] 5.1 Scheduler en el binario api: pre-encola la próxima ocurrencia con `run_at` = instante programado, `trigger_type: cron` y `trigger_data.scheduled_at`; test con reloj inyectable
- [ ] 5.2 Política de solape: ocurrencia con ejecución previa `pending|running` → no encola y registra `missed_triggers(overlap)`; test
- [ ] 5.3 Recuperación de caída: al arrancar encola solo la ocurrencia vencida más reciente y registra el resto como `downtime`; test simulando downtime
- [ ] 5.4 `POST /api/v1/bridges/{id}/run`: encola ejecución `manual` inmediata de la versión publicada activa y devuelve `executionId` (409 si no hay versión publicada); tests
- [ ] 5.5 Trigger cron no se expone en `/x`: puente cron publicado no registra ruta de invocación; test

## 6. Fase 6 — Cierre

- [ ] 6.1 E2E punta a punta: puente condicional (`switch` con dos rutas) disparado async → polling → webhook firmado recibido en servidor local; y puente cron con solape simulado
- [ ] 6.2 README: sección "MVP2" (async, polling, webhook, cron, condicionales) con ejemplos curl y verificación de firma
- [ ] 6.3 CI verde (`cargo test --workspace`, `clippy -D warnings`) y OpenAPI validado; commits por unidad de trabajo
