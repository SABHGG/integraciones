# Bloques y Puentes (nombre provisional)

[![CI](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml/badge.svg)](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml)

Plataforma de integraciones API-first: **bloques** (unidades de código JS/TS aisladas en sandbox o built-in `http`) encadenados en **puentes** (cadenas lineales disparables por HTTP). El MVP1 expone solo API; la UI (dashboard, formularios generados y editor de bloques) llega en el MVP3.

La especificación completa vive en [`openspec/`](openspec/) — empezar por `openspec/project.md` y las capacidades en `openspec/specs/`. El hito siguiente está documentado como change: `openspec/changes/add-mvp3-events-observability-ui/` (MVP1 y MVP2 ya construidos y archivados).

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

## MVP2: asíncrono, polling, webhook firmado, cron y condicionales

Todo lo de la sección anterior sigue igual; esto se añade por encima.

### Disparo asíncrono y polling

Con `Prefer: respond-async` (RFC 7240) el disparo responde `202` en cuanto la
ejecución queda encolada, con el `Location` del recurso de resultado:

```bash
curl -si -X POST localhost:8080/x/registro-cliente \
     -H 'content-type: application/json' \
     -H 'prefer: respond-async' \
     -d '{"items": ["cafe"]}'
# HTTP/1.1 202
# Location: /x/registro-cliente/executions/<executionId>
# {"success":true,"executionId":"..."}

curl -s localhost:8080/x/registro-cliente/executions/<executionId>
# en curso:  {"success":true,"executionId":"...","status":"running"}
# terminado: {"success":true,"data":{...},"executionId":"..."} (idéntico al síncrono)
```

La ejecución la corre el worker interno (cola `executions` con SKIP LOCKED).
`Idempotency-Key` funciona igual en async: la repetición devuelve `202` con el
`executionId` original. Los errores de transporte (404/405/413/415) no cambian.

### Webhook de resultado firmado

Si el borrador del puente declara `webhook: {"url": "https://..."}`, al
terminar cada ejecución — con éxito o con fallo — el sistema entrega el sobre
final a esa URL con:

- `X-Integraciones-Signature: sha256=<hex>` — HMAC-SHA256 **del body crudo**
  con el secreto del puente.
- `X-Integraciones-Timestamp` (unix) y `X-Integraciones-Delivery` (estable por
  entrega, para deduplicar reintentos).

El secreto se genera al publicar y solo se expone por el plano admin:

```bash
curl -s -H "$K" localhost:8080/api/v1/bridges/<id>/webhook-secret   # {"secret":"<64 hex>"}
```

Verificación en el receptor (Python):

```python
import hmac, hashlib
esperada = "sha256=" + hmac.new(SECRETO.encode(), body_crudo, hashlib.sha256).hexdigest()
assert hmac.compare_digest(esperada, request.headers["X-Integraciones-Signature"])
```

Reintentos persistentes con backoff fijo: 30 s → 2 min → 10 min → 1 h → 6 h
(5 intentos; éxito solo con 2xx). El estado vive en `webhook_deliveries`
(`pending → delivered | failed`, con `attempts`, `response_status`).

### Puentes cron

El trigger admite `{ "type": "cron", "schedule": "0 9 * * 1-5",
"timezone": "Europe/Madrid" }` (cron de 5 campos + zona IANA, validados al
publicar; horarios de verano respetados). El scheduler interno pre-encola cada
ocurrencia con `run_at` exacto; si una ejecución anterior sigue viva, la
ocurrencia se salta y queda en `missed_triggers` (`overlap`); tras una caída
se recupera solo la ocurrencia más reciente (`downtime` para el resto). Un
puente cron **no** se expone en `/x`. Para probarlo a demanda:

```bash
curl -s -X POST -H "$K" localhost:8080/api/v1/bridges/<id>/run
# 202 {"executionId":"..."}  (409 si el puente no tiene versión publicada)
```

### Puentes condicionales (`when` y `switch`)

Cada instancia puede declarar `when` — una condición **declarativa** sobre
refs `$.` (nada de JS) — y la cadena admite el bloque built-in `switch` con
rutas nombradas a instancias posteriores:

```json
{
  "id": "b-switch", "name": "enrutador", "blockId": "<switch>", "blockVersionId": "<v>",
  "input": {}, "config": {
    "routes": [
      { "when": { "gte": ["$.trigger.body.total", 10] }, "to": "b-vip" },
      { "when": { "eq": ["$.cliente.pais", "ES"] }, "to": "b-iberia" }
    ],
    "default": "b-normal"
  }
}
```

Gramática de condiciones: `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `exists`,
`contains`, combinables con `and` / `or` / `not`. La ruta no tomada queda en
el trace como `skipped` (en su `seq`, sin IO). El primer error en la ruta
tomada corta la ejecución (fail-fast, como siempre).

## Seguridad del sandbox

Los bloques `custom_logic` corren en un isolate QuickJS con límite de memoria
y timeout: sin red directa, sin procesos, sin módulos externos. El acceso al
mundo exterior pasa solo por `ctx.http.request` (timeout obligatorio, permite
`ctx.credential`), `ctx.log`, `ctx.crypto` y helpers JSON. Los secretos de la
bóveda se guardan cifrados (AES-256-GCM) y jamás quedan en el IO persistido
del trace.
