# Bloques y Puentes (nombre provisional)

Plataforma de integraciones API-first: **bloques** (unidades de código JS/TS aisladas en sandbox) encadenados en **puentes** (cadenas lineales disparables por HTTP, cron, eventos o manualmente). El MVP1 expone solo API; la UI (dashboard, formularios generados y editor de bloques) llega en el MVP3.

La especificación completa vive en [`openspec/`](openspec/) — empezar por `openspec/project.md` y el change `openspec/changes/add-mvp1-core/`.

## Estructura

| Crate | Rol |
|---|---|
| `crates/domain` | Tipos del dominio, refs `$.`, validador JSON Schema (subconjunto) |
| `crates/sandbox` | Wrapper rquickjs: isolate fresco, límites, host functions `ctx` |
| `crates/engine` | Ejecutor: cadena lineal, fail-fast, persistencia del trace |
| `crates/api` | Servidor axum: plano admin `/api/v1` + plano invocación `/x` |

## Desarrollo local

```bash
docker compose up -d          # Postgres (si docker está disponible)
cargo test --workspace        # pruebas
cargo run -p api              # servidor (puerto 8080)
```

### Variables de entorno (crear `.env` local, ver `.env.example` no versionado)

```
DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones
PORT=8080
RUST_LOG=info
```

> La migración `crates/api/migrations/0001_initial_schema.sql` se aplica sola al arrancar el servidor (`sqlx::migrate!`).

### Alta de una API key de administración

La biblioteca de bloques es por workspace, así que la key debe estar vinculada a uno
(`api_keys.workspace_id`): una key sin workspace autentica, pero recibe 403 en
`/api/v1/blocks`.

```bash
cargo run -p api -- hash-key "mi-key-secreta"   # imprime el SHA-256
psql "$DATABASE_URL" -c "
WITH ws AS (INSERT INTO workspaces (id, name, slug)
            VALUES (gen_random_uuid(), 'dev', 'dev') RETURNING id)
INSERT INTO api_keys (id, workspace_id, name, key_hash)
SELECT gen_random_uuid(), ws.id, 'dev', '<hash>' FROM ws;"
```

La auth del plano admin es `Authorization: Bearer mi-key-secreta`.
