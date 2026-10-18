# Proposal: add-mvp1-core

## Why

La plataforma de integraciones "Bloques y Puentes" existe hoy solo como diseño (9 rondas de decisiones documentadas). Necesitamos convertirlo en un primer producto ejecutable que valide las dos apuestas técnicas de mayor riesgo: la ejecución de bloques JS/TS en sandbox (rquickjs + quickjs-ng) y el ejecutor desacoplado con store de ejecuciones. Sin este corte vertical no hay forma de verificar que el modelo de dominio funciona contra APIs reales.

## What Changes

Entrega el **MVP1** completo, de cero a producto utilizable:

- **Sandbox**: wrapper rquickjs/quickjs-ng con isolate fresco por ejecución, límites de memoria, interrupt handler (timeout por puente en ms), host functions (`ctx.http.request`, `ctx.log`, `ctx.crypto`, helpers JSON, `ctx.credential`).
- **Ejecutor**: recorre la cadena lineal de bloques, resuelve referencias `$.` (raíces `$.trigger`/`$.now`), valida contra inputSchema por bloque, fail-fast en cualquier error, persiste IO por bloque.
- **Store de ejecuciones**: tabla `executions` (que también es la cola con SKIP LOCKED), `execution_steps` con IO completo, retención 30 días / 1 año.
- **Biblioteca de bloques**: CRUD + publicación de versiones inmutables (transpile TS→JS con swc al publicar) + bloques built-in `http-get`/`http-post`.
- **Gestión de puentes**: CRUD + `validate` + versiones inmutables (documento JSONB).
- **API de invocación**: `/x/{slug}` con method matching, `Idempotency-Key`, sync 200 con sobre `{success, data, error?, executionId}`, errores de transporte RFC 9457.
- **Bóveda mínima de credenciales**: CRUD write-only + resolución en host por referencia.
- **Identidad preparada**: tablas de users/workspaces/roles desde el día 1 (auth = API key global Bearer).

**Fuera de alcance (MVP2/MVP3)**: disparo async 202 + polling, webhook de resultado firmado, cron, eventos/colas, dry-run con mocks, replay, enmascaramiento de IO, UI.

## Impact

- **Affected specs**: nuevas capacidades `block-library`, `bridge-management`, `execution-engine`, `bridge-invocation`, `execution-store`, `credentials-vault` (todas ADDED).
- **Affected code**: todo el repositorio (greenfield). Estructura Cargo nueva, migraciones Postgres nuevas.
- **No afecta** nada existente (repo sin código).
- **Riesgo principal asumido**: rquickjs/quickjs-ng es la ruta crítica técnica; la Fase 1 de tareas lo valida primero con suite de bloques trampa.
