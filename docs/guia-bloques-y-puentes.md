# Guía: crear bloques y puentes

Cómo construir integraciones en la plataforma: primero un **bloque** (la pieza
reutilizable de la biblioteca) y después un **puente** (la cadena que los
encadena detrás de un disparador). Todo lo descrito aquí es operable desde la
consola (`/console`) o por API (`/api/v1`); la validación siempre es del
servidor: nada se congela hasta que valida.

## Quick path

1. **Crea un bloque** `custom_logic` (o usa los built-in `http`): `POST /api/v1/blocks` → publica su código con `POST /api/v1/blocks/{id}/versions`.
2. **Crea un puente**: `POST /api/v1/bridges` → arma el borrador con `PATCH /api/v1/bridges/{id}` (trigger + cadena de bloques) → `POST …/validate` → `POST …/versions` (publica la versión congelada).
3. **Invócalo**: disparador `http` ⇒ `POST /x/{slug-del-puente}`; verifica el trace en `/console/executions`.

En la consola: **Bloques → nuevo bloque**, **Puentes → editor → guardar/validar/publicar**, **ensayar (dry-run)** sin efectos, **versiones → diff/rollback**, **exportar/importar**.

## El bloque: la pieza de la biblioteca

Un bloque tiene un `slug`, metadatos y **versiones inmutables** (`latestVersion`
apunta a la vigente; publicar una versión nueva nunca toca las anteriores).

| Campo | Obligatorio | Notas |
|---|---|---|
| `slug` | ✓ | kebab-case, único en el workspace |
| `name` | ✓ | ≤ 200 caracteres |
| `blockType` | ✓ | `custom_logic` (los built-in `http` ya existen) |
| `category`, `description`, `icon` | — | filtrado y catálogo |

### Bloque built-in `http` (sin código)

El **host lo ejecuta** desde la `config` de la instancia: no lleva `code`. La
config soporta interpolación `{$.ruta}` y placeholders de credencial
`{$credential:nombre}` (el secreto solo vive en memoria del host; jamás en el
trace):

```json
{ "url": "https://api.proveedor.com/users/{$.trigger.body.userId}",
  "method": "GET",
  "headers": { "X-Api-Key": "{$credential:pago-api}" } }
```

### Bloque `custom_logic` (tu código)

```bash
# 1. crear el bloque
curl -X POST /api/v1/blocks -H "Authorization: Bearer $KEY" \
  -H 'content-type: application/json' \
  -d '{"slug": "enriquece-cliente", "name": "Enriquece cliente",
       "blockType": "custom_logic", "category": "transform",
       "description": "Pone el nombre en mayúsculas"}'

# 2. publicar la versión 1 (transpila TS→JS con swc y congela)
curl -X POST /api/v1/blocks/$BLOQUE/versions -H "Authorization: Bearer $KEY" \
  -H 'content-type: application/json' -d '{
    "code": "export default (input, ctx) => { const c = input.cliente ?? \"?\"; ctx.log(\"info\", \"enriqueciendo \" + c); return { cliente: c.toUpperCase() }; }",
    "inputSchema":  { "type": "object", "required": ["cliente"] },
    "outputSchema": { "type": "object", "required": ["cliente"] },
    "ui": { "title": "Enriquece cliente",
            "order": ["mayusculas"],
            "widget": { "mayusculas": "text" },
            "help":   { "mayusculas": "Fuerza mayúsculas en el nombre" } }
  }'
```

**El código**: un único `export default (input, ctx) => salida`. `input` es el
mapeo `input` de la instancia ya resuelto; la **salida debe cumplir el
`outputSchema`** (verificación estática al publicar). Con un error de
transpila, la publicación se bloquea y el validador señala la línea (visible
en el editor de la consola).

**La `ui`** (opcional, solo metadatos): genera los formularios de la consola.

| Clave | Forma | Efecto |
|---|---|---|
| `title` | string | título del formulario |
| `order` | `["campo", …]` | orden de los campos |
| `widget` | `{campo: id}` | `text`, `url`, `number`, `textarea`, `password` (desconocido ⇒ texto) |
| `help` | `{campo: texto}` | ayuda bajo el campo |

**El sandbox** (QuickJS): límite de memoria y `timeoutMs` por instancia; sin
red directa, procesos ni módulos. Lo que sí tienes:

| API | Uso |
|---|---|
| `ctx.http.request({url, method, headers, body})` | la única red; `url` obligatorio; el host puede restringir dominios |
| `ctx.credential("nombre")` | valor descifrado de la bóveda; nunca entra al IO persistido |
| `ctx.log("info", "…")` | logs del trace |
| `ctx.crypto.sha256(s)` / `ctx.crypto.hmacSha256(clave, s)` / `base64Encode` / `base64Decode` | utilidades de firma y codificación |
| `ctx.json.parse(s)` / `ctx.json.stringify(v)` | JSON seguro |
| `input` | entrada resuelta (ver refs abajo) — el bloque ve SOLO su `input`, no el contexto completo |

## El puente: la cadena

Un puente = **disparador + cadena lineal de instancias de bloque**. El flujo
siempre es el mismo: borrador → validar → **publicar versión congelada** (las
ejecuciones corren contra ese snapshot exacto; editar después no las afecta).

```bash
curl -X POST /api/v1/bridges -H "Authorization: Bearer $KEY" \
  -H 'content-type: application/json' \
  -d '{"slug": "registro-cliente", "name": "Registro de cliente"}'

curl -X PATCH /api/v1/bridges/$PUENTE -H "Authorization: Bearer $KEY" \
  -H 'content-type: application/json' -d '{ "trigger": …, "blocks": […] }'

curl -X POST /api/v1/bridges/$PUENTE/validate -H "Authorization: Bearer $KEY"   # errores en línea
curl -X POST /api/v1/bridges/$PUENTE/versions -H "Authorization: Bearer $KEY"   # publica v1
```

### Disparadores (`trigger`)

| Tipo | Forma | Invocación |
|---|---|---|
| `http` | `{"type":"http","method":"POST","path":"/x/registro-cliente"}` | `POST /x/{slug}` (body ⇒ `$.trigger.body`) |
| `cron` | `{"type":"cron","schedule":"0 9 * * 1-5","timezone":"Europe/Madrid"}` | scheduler (5 campos, sin solapes) |
| `event` | `{"type":"event","topic":"cliente.creado"}` | `POST /api/v1/events/{topic}` (fan-out) |

> **Importante**: la invocación http despacha por **slug del puente** —
> `POST /x/{slug}`. La convención es `path = "/x/{slug}"` (el validador solo
> exige que exista y empiece por `/`, pero una ruta que no coincida con el
> slug despacha 404).

`inputSchema` (opcional) valida el body entrante en el plano de invocación.

### Instancias (`blocks[]`)

```json
{
  "id": "b1", "name": "traeUsuario",
  "blockId": "…", "blockVersionId": "…",
  "input":  { "usuario": "$.trigger.body.usuario" },
  "config": { "url": "https://httpbin.org/get", "method": "GET" },
  "when":   { "exists": "$.trigger.body.usuario" },
  "timeoutMs": 5000
}
```

- `name`: cómo se registra la salida (`$.traeUsuario…`). Único en la cadena.
- `input`: mapeo de entrada (valores literales o referencias `$.`).
- `config`: solo para built-in `http` (interpolación `{$.ruta}`, `{$credential:x}`).
- `code`: solo para `custom_logic` inline (normalmente viaja en la versión del bloque).
- `when` (opcional): condición declarativa; falsa ⇒ paso `skipped` sin ejecutar.

### Referencias `$.` (el pegamento)

| Referencia | Resuelve a |
|---|---|
| `$.trigger.body.campo` | body del disparo http (`$.trigger` completo en cron/event) |
| `$.<nombreBloque>.<campo>` | salida de un bloque anterior |
| `$.now` | timestamp del disparo (raíces `trigger` y `now` reservadas) |

Una referencia que no resuelve es **fail-fast** (la ejecución falla con el
paso exacto en el trace): no hay silencios.

### Condiciones (`when` y rutas de `switch`)

Declarativas, evaluadas en el host (sin código del usuario):

```json
{ "and": [ { "gte": ["$.cliente.pedidos", 10] }, { "eq": ["$.cliente.pais", "ES"] } ] }
```

Operadores: `eq`, `ne`, `gt`/`gte`/`lt`/`lte`, `exists`, `contains`;
combinadores `and`/`or`/`not`. Referencia no resuelta o tipos incomparables ⇒
fallo determinista, nunca una ruta elegida en silencio.

### Al terminar: `emit`, `webhook`, `mask`

| Sección | Forma | Efecto |
|---|---|---|
| `emit` | `{"onSuccess": ["cliente.creado"], "onFailure": ["cliente.fallo"]}` | publica eventos al completar (encadena puentes por topic) |
| `webhook` | `{"url": "https://tuservidor/hook"}` | POST firmado con `X-Integraciones-Signature` (HMAC-SHA256), `X-Integraciones-Timestamp` y `X-Integraciones-Delivery`, con reintentos |
| `mask` | `["$.trigger.body.tarjeta", "$.creaOrden.rest.tarjeta"]` | `***` irreversible ANTES de persistir (trace, listado, webhook ya enmascarados) |

## Checklist antes de publicar

- [ ] El `outputSchema` del bloque coincide con lo que tu código devuelve
- [ ] Toda referencia `$.` apunta a un bloque ANTERIOR o a `trigger`/`now`
- [ ] `inputSchema` del puente valida el body que esperas (si el trigger es http)
- [ ] `POST …/validate` responde `"valid": true` (o errores en línea resueltos)
- [ ] Dry-run con mocks verificado en la consola (sin filas ni webhook)
- [ ] Datos sensibles bajo `mask` si van a aparecer en el trace

## Ver también

- Bóveda versionada, dry-run con mocks y replay: README, sección MVP3
- Roles y auditoría: README, sección MVP4
- Contrato completo: `/docs` (Scalar) o `openapi.json`
