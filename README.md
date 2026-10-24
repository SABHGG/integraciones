# Bloques y Puentes (nombre provisional)

[![CI](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml/badge.svg)](https://github.com/SABHGG/integraciones/actions/workflows/ci.yml)

Plataforma de integraciones API-first: **bloques** (unidades de código JS/TS aisladas en sandbox o built-in `http`) encadenados en **puentes** (cadenas lineales disparables por HTTP). El MVP1 expone solo API; la UI (dashboard, formularios generados y editor de bloques) llega en el MVP3.

La especificación completa vive en [`openspec/`](openspec/) — empezar por `openspec/project.md` y las capacidades en `openspec/specs/`. Todos los hitos están construidos y archivados (MVP1–MVP6, incluido el sistema visual El rastro de la consola); no hay changes activos.

## Estructura

| Crate | Rol |
|---|---|
| `crates/core` | Tipos del dominio, refs `$.`, validador JSON Schema (subconjunto) |
| `crates/sandbox` | Wrapper rquickjs: isolate fresco, límites, host functions `ctx` |
| `crates/engine` | Ejecutor: cadena lineal, fail-fast, persistencia del trace |
| `crates/api` | Servidor axum: plano admin `/api/v1` + plano invocación `/x` |

## Documentación

| Guía | Para qué |
|---|---|
| [`docs/guia-consola.md`](docs/guia-consola.md) | Entrar, las cinco páginas, roles, leer un trace, flujos diarios, sistema visual El rastro y sus tokens |
| [`docs/guia-bloques.md`](docs/guia-bloques.md) | Crear bloques: built-in `http` vs `custom_logic`, contrato del código, sandbox `ctx.*`, metadatos `ui`, versiones inmutables |
| [`docs/guia-puentes.md`](docs/guia-puentes.md) | Crear puentes: disparadores, refs `$.`, `when`/`switch`, emit/webhook/mask, dry-run, publicar e invocar |
| [`docs/errores-comunes.md`](docs/errores-comunes.md) | Diagnóstico rápido: 401/403/404/409/422, códigos de ejecución (`REF_UNRESOLVED`, `TIMEOUT`…), webhooks y eventos |
| [`docs/diagramas/`](docs/diagramas/) | Diagramas interactivos (arquitectura, invocación, publicación, ciclo de vida, rastro de datos) con su fuente JSON y recibos de verificación |

## Desarrollo local

```bash
./scripts/dev-postgres.sh     # Postgres portátil en 127.0.0.1:5433 (sin docker)
cargo test --workspace        # pruebas (la API y el engine necesitan DATABASE_URL)
./scripts/dev.sh              # todo en uno: postgres + .env + consola + servidor + seed demo
./scripts/dev.sh --console-watch   # además reconstruye la consola al editar console/src
```

`dev.sh` deja la API en `http://127.0.0.1:8080` con la consola en `/console` (login con la API key del workspace; con el seed: `demo-key-1`). En DEBUG, rust-embed lee `console/dist/` del disco: tras editar la consola basta `npm run build` (o el watch) y refrescar el navegador — sin recompilar el binario.

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

### Eventos: suscripción por topic, fan-out y `emit` (MVP3)

Un puente puede suscribirse a un topic (`{"trigger": {"type": "event", "topic": "pedido.creado"}}`) y publicarse eventos por API (`POST /api/v1/events/pedido.creado` con el payload como body; `202` con el número de ejecuciones encoladas, cero si no hay suscriptores — el evento no se retiene). Un puente también puede emitir eventos al terminar (`"emit": {"onSuccess": ["topic"], "onFailure": ["topic"]}`) con el sobre final como payload, enriquecido con `bridgeSlug` y `executionId` del emisor: así se encadenan puentes sin acoplarlos.

**Límites documentados**: el fan-out no garantiza orden ni entrega entre suscriptores — cada suscriptor es una ejecución independiente con su propio fail-fast y trace; si un suscriptor no tiene versión publicada en el instante del fan-out, pierde ese evento. No hay cola de eventos persistida: el evento ES el conjunto de ejecuciones creadas (D1). Un topic con cientos de suscriptores alarga la transacción de publicación: es un límite práctico conocido.

### Bóveda versionada, mask, dry-run, replay y consola (MVP3)

**Bóveda versionada con rotación**: `POST /api/v1/credentials` crea la versión 1; `PATCH` con `secret` crea una versión inmutable y mueve la vigente — los puentes no se tocan. `GET /api/v1/credentials/{id}/versions` lista el historial (ids y fechas, jamás valores) y cada ejecución audita en `credentialsUsed` qué versión usó (verificable contra el historial).

**Enmascaramiento declarativo**: añade `"mask": ["$.trigger.body.tarjeta", "$.capturar.recibido.tarjeta"]` a la definición (raíces `trigger` o nombre de bloque; congelado por versión). Todo valor bajo esas rutas se sustituye por `***` **antes de persistir** — irreversible, sin flag de revelado: trace, listado, detalle y webhook entregan el dato ya enmascarado. Editar las reglas solo afecta a ejecuciones posteriores.

**Dry-run sin efectos**: `POST /api/v1/bridges/{id}/dry-run` ejecuta el borrador actual (sin publicar) con datos de prueba y devuelve el trace completo en la respuesta — sin filas en `executions`, sin webhook y sin `emit`. Por instancia puedes declarar `"mocks": {"capturar": {"output": {...}}}`: el bloque mockeado no se invoca y su salida fija alimenta la cadena; los demás bloques corren de verdad.

```bash
curl -X POST /api/v1/bridges/$ID/dry-run \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"triggerData": {"total": 7}, "mocks": {"capturar": {"output": {"total": 42}}}}'
```

**Replay controlado**: `POST /api/v1/executions/{id}/replay` re-ejecuta con la misma versión congelada y la misma entrada (con `replayOf` en `triggerData`), por la cola normal — mismo masking, mismo webhook. El detalle del original lista sus replays; el del replay enlaza al original.

**Consola web**: el mismo binario sirve una SPA en `/console` — dashboard de ejecuciones con trace paso a paso, editor visual de puentes (formularios generados desde los metadatos `ui` de cada bloque, errores del validador sobre el canvas, editor de código con los errores de transpile), dry-run con un clic y publicación. Sin backend nuevo: la consola habla exclusivamente con el plano admin. Entra con **email y contraseña** (sesión de usuario) o con tu API key (acceso de automatización/demo).

## Usuarios, roles y miembros (MVP4)

**Cuentas y sesiones**: `POST /api/v1/auth/register` crea la cuenta (contraseña ≥ 10, hash argon2id; error uniforme 401 en login para no revelar si el email existe). El registro no da acceso a ningún workspace: hace falta que un owner añada a la persona por email. `POST /api/v1/auth/login` devuelve un token opaco (guardamos solo su SHA-256) — con una membresía la sesión nace fijada a ese workspace; con varias exige `"workspace": "<slug>"` (409 para elegir). `POST /api/v1/auth/logout` revoca la sesión en el servidor.

```bash
curl -X POST /api/v1/auth/login -H "Content-Type: application/json" \
  -d '{"email":"edu@dominio","password":"…","workspace":"mi-ws"}'
# → {"token":"…","activeWorkspace":{"slug":"mi-ws","role":"editor",…},…}
```

**Roles declarativos**: `owner > editor > runner > viewer`. La matriz vive en el middleware (`rol_requerido`): lectura = viewer, disparos (run/dry-run/replay/eventos) = runner, escritura de recursos (puentes, bloques, credenciales, publicaciones) = editor, membresía = owner. Las API keys actúan como `owner` (compatibilidad con automatización y CI). El rol se resuelve **por petición**: un cambio de rol o una expulsión surte efecto al instante, sin esperar expiración.

**Miembros** (owner; la lista la ve cualquier identidad autenticada): añadir por email de cuenta existente (`POST /api/v1/members`, 404 claro si aún no se registró, 409 si ya es miembro), cambiar rol (`PATCH /api/v1/members/{userId}`) y expulsar (`DELETE`). El **último owner está protegido**: 409 al degradarlo o expulsarlo hasta que haya otro.

```bash
curl -X POST /api/v1/members -H "Authorization: Bearer $TOKEN_OWNER" \
  -H "Content-Type: application/json" \
  -d '{"email":"edu@dominio","role":"editor"}'   # 201
```

**Auditoría del disparador**: cada ejecución guarda `triggerPrincipal` — `{"type":"user","email":…}` en disparos y replays de usuario, `{"type":"api_key","name":…}` con keys y `{"type":"system","motivo":"cron"|"emit"}` en los internos (`null` en el ingestor sin identidad). La consola refleja el rol del `whoami`: oculta lo que el rol no permite y muestra los 403 del servidor donde ocurren; la página **Miembros** es editable solo para owners y de lectura para el resto.

## La consola completa: credenciales, bloques, versiones, métricas y transferencia (MVP5)

Todo lo que el plano sabe hacer es ahora operable desde `/console` — sin endpoints de negocio nuevos: solo dos lecturas que faltaban (`GET /stats`, definición congelada de una versión) y orquestación en cliente sobre el plano existente. Cada página respeta la matriz de roles: el rol que puede operar va entre paréntesis.

**Credenciales (editor; viewer lee)**: listado con versión vigente, creación, rotación en línea e historial por credencial (ids y fechas, jamás valores). La eliminación exige **teclear el nombre** — irreversible no se confirma con un clic.

**Biblioteca de bloques (editor crea/publica; viewer inspecciona)**: listado con filtros combinables (texto, tipo, categoría), detalle de versiones con sus schemas y metadatos `ui`, y creación/edición de `custom_logic` con el editor de código: un error de transpile anota la línea del validador y bloquea la publicación.

**Versiones con diff y rollback (diff para todos; rollback editor)**: cada puente lista sus versiones y compara lado a lado —versión contra versión o borrador contra versión— con un diff estructural por rutas (`blocks.0.config.url`), en cliente y sin dependencias. El **rollback** re-aplica la definición congelada al borrador y la publica como versión nueva (confirmación en dos pasos, etiquetado «rollback de la N»): el pasado jamás se reescribe, las ejecuciones históricas siguen ancladas a su versión.

**Métricas y auto-refresh (lectura)**: el dashboard muestra total, estados, duración media y tendencia diaria con huecos rellenados para un rango de 7/30/90 días (`GET /stats`, una sola consulta agregada, máx. 90 días), con desglose por puente navegable al listado filtrado. El listado ofrece **auto-refresh** por polling cada 5 s — apagado por defecto, pausado con la pestaña oculta y sin peticiones solapadas.

**Export/import/duplicar (export para todos; importar y duplicar editor)**: exporta un puente (borrador o versión congelada) como `{name, slug, definition}` sin ids de workspace; impórtalo en cualquier workspace pegando el JSON — el plano valida (errores en línea) y la publicación es una casilla explícita que jamás publica un borrador inválido. Duplicar crea `slug-copia` con el borrador listo en un clic.

## Seguridad del sandbox

Los bloques `custom_logic` corren en un isolate QuickJS con límite de memoria
y timeout: sin red directa, sin procesos, sin módulos externos. El acceso al
mundo exterior pasa solo por `ctx.http.request` (timeout obligatorio, permite
`ctx.credential`), `ctx.log`, `ctx.crypto` y helpers JSON. Los secretos de la
bóveda se guardan cifrados (AES-256-GCM) y jamás quedan en el IO persistido
del trace.
