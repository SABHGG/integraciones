# Project: Bloques y Puentes (nombre provisional)

## Project Overview

Plataforma headless de integraciones propia (no es n8n/Make/Zapier). Dos conceptos centrales:

- **Bloque**: unidad de código ejecutable aislado. Biblioteca reutilizable versionada inmutablemente.
- **Puente (bridge)**: cadena **lineal** de instancias de bloques que ejecutan una acción. Se dispara por HTTP, cron, evento o manualmente, y produce un registro de ejecución observable.

El backend está escrito en **Rust** y ejecuta los bloques (JavaScript/TypeScript) dentro de **V8-likes: QuickJS vía rquickjs (quickjs-ng)** — un isolate fresco por ejecución.

## Current State

Fase de diseño completada. **No existe código aún.** Toda la base de conocimiento vive en:

- `openspec/changes/add-mvp1-core/` — propuesta, decisiones de diseño y tareas del MVP1
- Memoria Engram del proyecto (topic_keys `integraciones-*`) — acta de las 9 rondas de decisiones

Próximo hito: MVP1 (ver `changes/add-mvp1-core/tasks.md`).

## Key Decisions (resumen ejecutivo)

| Área | Decisión |
|---|---|
| Disparadores | HTTP, cron, eventos/colas, manual (los 4; cron/eventos en hitos posteriores) |
| Sandbox | rquickjs + quickjs-ng; isolate fresco por ejecución; límites de memoria + interrupt handler |
| Lenguaje de bloques | Solo JS/TS; TS transpilado con swc **al publicar** (versión inmutable guarda JS) |
| Convención de bloque | `export default function(input, ctx) → salida \| Promise`; metadatos en `BlockDefinition`, config en la instancia |
| Capacidades de `ctx` | `ctx.http.request` (timeout obligatorio, allowlist de dominios), `ctx.log`, `ctx.crypto`, helpers JSON, `ctx.credential(name)`. Sin npm en v1 |
| Errores | Fail-fast simple; bloque señala error con `throw` nativo |
| Respuesta HTTP del puente | Sync: 200 con sobre `{success, data, error?, executionId}`; async: 202 + `{success, executionId}`. Códigos de error estables |
| Datos entre bloques | JSON con schemas estrictos; contexto acumulado con salidas nombradas; referencias con prefijo `$.` (raíces `$.trigger`, `$.now`); ref no resuelta = fail-fast |
| Observabilidad | IO completo por bloque persistido + replay (MVP2); trazas en `execution_steps` |
| Secretos | Bóveda compartida por referencia (resuelta en host, nunca en IO); enmascaramiento como red de seguridad; retención configurable (~30 días IO) |
| Tenancy | Workspaces con roles (owner/editor/runner/viewer); auth v1 = API key global (Bearer), schema preparado para identidad real |
| Versionado | Versiones inmutables; ejecuciones guardan snapshot de la definición |
| Persistencia | Postgres único (JSONB + cola con SKIP LOCKED); cola abstracta para Redis futuro |
| Despliegue | Binario único + Postgres; workers stateless; preparado para escalar sin reestructurar |
| Idempotencia | `Idempotency-Key` opcional en disparo HTTP |
| Testing | Dry-run con datos de prueba + bloques mock (MVP3) |
| API | Dos planos: `/api/v1` (admin, RFC 9457) y `/x/{slug}` (invocación); Bearer; paginación por cursor; UUID v7 + slug |

## Architecture

```
┌────────────┐  /x/{slug}   ┌─────────────────────────────────────┐
│  Llamador  │──────────────►  API Rust (axum)                   │
└────────────┘              │  • plano admin  /api/v1             │
                            │  • plano invocación /x              │
                            └───────┬─────────────────────────────┘
                                    │ ejecutor desacoplado
                        ┌───────────▼────────────┐
                        │  Cola: executions      │  (Postgres SKIP LOCKED)
                        │  status=pending+run_at │
                        └───────────┬────────────┘
                                    │
                        ┌───────────▼────────────┐
                        │ Worker: ejecutor       │
                        │  • resuelve refs $.    │
                        │  • valida schemas      │
                        │  • invoca rquickjs     │
                        │    isolate fresco      │──► ctx.http / ctx.log /
                        │  • persiste IO         │    ctx.crypto / ctx.credential
                        └───────────┬────────────┘
                                    ▼
                     Postgres: blocks, block_versions, bridges,
                     bridge_versions, executions, execution_steps,
                     webhook_deliveries, missed_triggers, credentials
```

## Stack

- **Backend**: Rust (candidatos: axum, sqlx, swc para transpile, rquickjs para sandbox)
- **BD**: PostgreSQL (JSONB, SKIP LOCKED)
- **Bloques**: JS/TS transpilados a JS

## Commands

```
openspec validate --all       # validar especificaciones
npx @fission-ai/openspec ...  # CLI sin instalación global
docker compose up -d          # Postgres (cuando exista compose)
```

## Roadmap

- **MVP1** — ejecutor + HTTP sync + store de ejecuciones + biblioteca de bloques + bóveda mínima
- **MVP2** — async 202 + polling + webhook de resultado (HMAC) + cron
- **MVP3** — eventos/colas, dry-run con mocks, replay, bóveda completa (rotación), enmascaramiento, UI (formularios + editor de código)
