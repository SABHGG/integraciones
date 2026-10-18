# Proposal: add-mvp2-async-cron-conditionals

## Why

El MVP1 solo dispara puentes por HTTP síncrono: el llamador espera a que la cadena completa termine y cualquier puente lento lo bloquea. El roadmap (MVP2) exige la segunda oleada de disparadores — asíncrono con polling, webhook de resultado firmado y cron — además de saltos condicionales dentro de la cadena, hoy imposible porque el ejecutor recorre los bloques estrictamente en orden.

## What Changes

- **Disparo asíncrono**: invocación `POST /x/{slug}` en modo async que responde `202` con `{success, executionId}` y `Location` del recurso de resultado; el puente (o la petición) elige el modo.
- **Polling de resultado**: endpoint de lectura en el plano de invocación que devuelve el sobre final (`{success, data, error?, executionId}`) cuando la ejecución termina.
- **Webhook de resultado firmado**: entrega HMAC-SHA256 del sobre final a la URL registrada, con reintentos exponenciales, estados persistidos en `webhook_deliveries` y verificación por firma.
- **Trigger cron**: definición de schedule (cron estándar) en el trigger del puente, cálculo de `run_at`, política de solapamiento (skip + registro) y recuperación de disparos perdidos tras caída (`missed_triggers`), con disparo manual "ejecutar ahora".
- **Puentes condicionales**: `when` por instancia o bloque `switch` con rutas nombradas; los pasos no ejecutados se registran como `skipped`; condiciones deterministas sobre `$.` (sin JS arbitrario) y prevención estructural de ciclos (la cadena sigue siendo un DAG lineal por ruta).

## Capabilities

### New Capabilities
- `async-invocation`: disparo async 202 + polling del resultado en el plano `/x`.
- `webhook-delivery`: entrega firmada del resultado del puente (HMAC) con reintentos y estados de entrega.
- `cron-triggers`: programación de puentes por schedule, solapamiento y disparos perdidos.

### Modified Capabilities
- `bridge-management`: el trigger admite `cron` (además de `http`); la definición admite `when`/`switch` con validación en tiempo de diseño (refs deterministas, rutas alcanzables, sin ciclos).
- `execution-engine`: el ejecutor evalúa condiciones deterministas sobre `$.`, enruta por `switch`, marca pasos no ejecutados como `skipped` y mantiene fail-fast por ruta.
- `execution-store`: `execution_steps.status` admite `skipped`; se registran `missed_triggers` y `webhook_deliveries`; `executions.webhook_url` pasa a usarse.

## Impact

- **Affected specs**: `async-invocation`, `webhook-delivery`, `cron-triggers` (ADDED); `bridge-management`, `execution-engine`, `execution-store` (MODIFIED).
- **Affected code**: `crates/api` (router `/x`, nuevo job scheduler cron, job de entregas webhook), `crates/engine` (evaluación de condiciones + `skipped`), migración nueva (`execution_steps.status` + índices de scheduler/webhook), `crates/domain` (gramática de condiciones).
- **Compatible hacia atrás**: el modo síncrono 200 y el contrato actual no cambian; sin `when`/`switch` el puente se comporta exactamente igual que en MVP1.
- **Fuera de alcance (MVP3)**: eventos/colas, dry-run con mocks, replay, rotación de bóveda, enmascaramiento, UI.
