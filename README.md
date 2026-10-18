# Bloques y Puentes (nombre provisional)

[![CI](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml/badge.svg)](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml)

Plataforma de integraciones API-first: **bloques** (unidades de código JS/TS aisladas en sandbox o built-in `http`) encadenados en **puentes** (cadenas lineales disparables por HTTP). El MVP1 expone solo API; la UI (dashboard, formularios generados y editor de bloques) llega en el MVP3.

La especificación completa vive en [`openspec/`](openspec/) — empezar por `openspec/project.md` y las capacidades en `openspec/specs/`. Los hitos siguientes están documentados como changes: `openspec/changes/add-mvp2-async-cron-conditionals/` y `add-mvp3-events-observability-ui/`.

## Estructura

| Crate | Rol |
|---|---|
| `crates/domain` | Tipos del dominio, refs `$.`, validador JSON Schema (subconjunto) |
| `crates/sandbox` | Wrapper rquickjs: isolate fresco, límites, host functions `ctx` |
| `crates/engine` | Ejecutor: cadena lineal, fail-fast, persistencia del trace |
| `crates/api` | Servidor axum: plano admin `/api/v1` + plano invocación `/x` |

## Desarrollo local

```bash
./scripts/dev-postgres.sh     # Postgres portátil en 127.0.0.1:5433 (sin docker)
cargo test --workspace        # pruebas (la API y el engine necesitan DATABASE_URL)
cargo run -p api              # servidor (puerto 8080)
```

### Variables de entorno (crear `.env` local)

```
DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones
PORT=8080
RUST_LOG=info
VAULT_KEY=<64 hex; sin ella la bóveda usa una clave efímera por proceso>
RETENTION_STEPS_DAYS=30          # purga del IO de execution_steps
RETENTION_EXECUTIONS_DAYS=365    # purga de los resúmenes de executions
RETENTION_INTERVAL_SECS=86400    # cada cuánto corre el job de retención
```

> La migración `crates/api/migrations/0001_initial_schema.sql` se aplica sola al arrancar el servidor (`sqlx::migrate!`). El job de retención arranca con el servidor y purga por `created_at`: primero el IO (ventana corta) y después los resúmenes (ventana larga).

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

La auth del plano admin es `Authorization: Bearer mi-key-secreta`. Los errores son Problem Details ([RFC 9457](https://www.rfc-editor.org/rfc/rfc9457), `application/problem+json`).

## El primer puente en cinco llamadas

Con el servidor corriendo y `K='Authorization: Bearer mi-key-secreta'`:

**1. Bóveda** (opcional: para headers con secretos):

```bash
curl -s -X POST localhost:8080/api/v1/credentials -H "$K" -H 'content-type: application/json' \
  -d '{"name": "proveedor", "secret": "sk_…"}'
```

**2. Biblioteca**: crear bloques y publicar su versión inmutable. Los `http` no llevan
código (su comportamiento es del ejecutor); los `custom_logic` publican TypeScript que
se transpila y valida al instante:

```bash
GET_ID=$(curl -s -X POST localhost:8080/api/v1/blocks -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "http-get", "name": "HTTP GET", "blockType": "http"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')

curl -s -X POST "localhost:8080/api/v1/blocks/$GET_ID/versions" -H "$K" -H 'content-type: application/json' \
  -d '{}'   # versión sin código: solo schemas
```

Un `custom_logic` cumple el contrato `export default (input, ctx) => salida`:

```js
export default (input, ctx) => {
  ctx.log("info", "procesando " + input.usuario.nombre);
  return { cliente: input.usuario.nombre.toUpperCase() };
};
```

**3. Puente**: borrador con trigger + cadena; cada instancia referencia el
`blockId` y la `blockVersionId` exacta. La entrada usa refs `$.` (salidas de
bloques anteriores por su `name` y `$.trigger.body`) y la config admite
interpolación `{$.ref}` y secretos `{$credential:nombre}`:

```bash
BRIDGE_ID=$(curl -s -X POST localhost:8080/api/v1/bridges -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "registro", "name": "Registro de cliente"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')

curl -s -X PATCH "localhost:8080/api/v1/bridges/$BRIDGE_ID" -H "$K" -H 'content-type: application/json' -d '{
  "trigger": { "type": "http", "method": "POST", "path": "/x/registro" },
  "blocks": [
    { "id": "b1", "name": "traeUsuario", "blockId": "<blockId>", "blockVersionId": "<versionId>",
      "config": { "url": "https://api.ejemplo.com/users/{$.trigger.body.id}", "method": "GET",
                  "headers": { "X-Api-Key": "{$credential:proveedor}" } } },
    { "id": "b2", "name": "enriquece", "blockId": "<blockId>", "blockVersionId": "<versionId>",
      "input": { "usuario": "$.traeUsuario" } }
  ]
}'
```

**4. Validar y publicar** (publicar resuelve los bloques, revalida y congela un
snapshot inmutable `v1`):

```bash
curl -s -X POST "localhost:8080/api/v1/bridges/$BRIDGE_ID/validate" -H "$K"
curl -s -X POST "localhost:8080/api/v1/bridges/$BRIDGE_ID/versions" -H "$K"
```

**5. Invocar**: el plano `/x/{slug}` no lleva auth de administración (así se
expone a clientes); responde `200` con sobre `{success, data, error?,
executionId}` y códigos de error estables (`BLOCK_FAILED`, `TIMEOUT`,
`VALIDATION_FAILED`, `REF_UNRESOLVED`):

```bash
curl -s -X POST localhost:8080/x/registro -H 'content-type: application/json' -d '{"id": 7}'
```

**Observabilidad**: cada ejecución queda persistida con su entrada, estado y
referencia a la versión exacta que corrió; el trace trae IO completo por bloque:

```bash
curl -s -H "$K" localhost:8080/api/v1/executions                      # listado cursor
curl -s -H "$K" localhost:8080/api/v1/executions/<id>                 # detalle
curl -s -H "$K" localhost:8080/api/v1/executions/<id>/trace           # IO por bloque
```

## Puente demo de un comando

`scripts/seed-demo.sh` siembra (idempotente) un puente real de 3 bloques —
`http-get → custom_logic → http-post` — con bóveda incluida:

```bash
./scripts/seed-demo.sh   # con el servidor corriendo (BASE_URL para otro puerto)
curl -s -X POST localhost:8080/x/registro-cliente \
     -H 'content-type: application/json' -d '{"items": ["cafe", "mate"]}'
```

## Seguridad del sandbox

Los bloques `custom_logic` corren en un isolate QuickJS con límite de memoria
y timeout: sin red directa, sin procesos, sin módulos externos. El acceso al
mundo exterior pasa solo por `ctx.http.request` (timeout obligatorio, permite
`ctx.credential`), `ctx.log`, `ctx.crypto` y helpers JSON. Los secretos de la
bóveda se guardan cifrados (AES-256-GCM) y jamás quedan en el IO persistido
del trace.
