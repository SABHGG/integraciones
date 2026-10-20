# Guía: crear y publicar puentes

Un **puente** es un disparador + una **cadena lineal de instancias de bloque**.
El flujo siempre es el mismo: borrador → validar → **publicar versión
congelada** (las ejecuciones corren contra ese snapshot exacto; editar después
no las afecta). Esta guía cubre disparadores, la cadena, referencias,
condiciones, ensayo y publicación. Todo es operable desde la consola
(**Puentes → editor**) o por API (`/api/v1`).

## Quick path

1. **Crea el puente**: `POST /api/v1/bridges` (slug + nombre) — o *Puentes → importar/duplicar* en la consola.
2. **Arma el borrador**: `PATCH /api/v1/bridges/{id}` con `trigger` + `blocks[]` (+ `inputSchema`, `emit`, `webhook`, `mask`).
3. **Valida y publica**: `POST …/validate` (errores en línea) → `POST …/versions` (congela la v1).
4. **Dispara y observa**: `POST /x/{slug}` (o cron/evento) → trace en **Ejecuciones**.

## El disparador (`trigger`)

| Tipo | Forma | Invocación |
|---|---|---|
| `http` | `{"type":"http","method":"POST","path":"/x/registro-cliente"}` | `POST /x/{slug}` — el body llega como `$.trigger.body` |
| `cron` | `{"type":"cron","schedule":"0 9 * * 1-5","timezone":"Europe/Madrid"}` | el scheduler dispara (5 campos, sin solapes) |
| `event` | `{"type":"event","topic":"cliente.creado"}` | `POST /api/v1/events/{topic}` (fan-out a todos los suscritos) |

> **La ruta despacha por slug**: `POST /x/{slug}`. El validador solo exige que
> `path` exista y empiece por `/`, pero una ruta que no coincida con el slug
> despacha 404. La convención es `path = "/x/{slug}"`.

`inputSchema` (opcional) valida el body entrante **en el plano de invocación**:
una entrada inválida falla con `VALIDATION_FAILED` antes de tocar ningún bloque.

## La cadena (`blocks[]`)

Cada elemento es una **instancia**: un bloque concreto de la biblioteca, en una
posición, con su entrada mapeada.

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

| Campo | Para qué |
|---|---|
| `name` | cómo se registra la salida (`$.traeUsuario…`); único en la cadena |
| `blockId` + `blockVersionId` | el bloque de la biblioteca y su versión congelada exacta |
| `input` | mapeo de entrada: valores literales o referencias `$.` |
| `config` | solo para built-in `http` (interpolación `{$.ruta}`, `{$credential:x}`) |
| `when` | condición declarativa; falsa ⇒ paso `skipped` sin ejecutar |
| `timeoutMs` | techo de tiempo del bloque (el sandbox lo corta con `TIMEOUT`) |

### Referencias `$.` (el pegamento)

| Referencia | Resuelve a |
|---|---|
| `$.trigger.body.campo` | body del disparo http (`$.trigger` completo en cron/event) |
| `$.<nombreBloque>.<campo>` | salida de un bloque **anterior** |
| `$.now` | timestamp del disparo (raíces `trigger` y `now` reservadas) |

Una referencia que no resuelve es **fail-fast**: la ejecución falla con el
paso exacto señalizado en el trace. Nunca una cadena silenciosa de `undefined`.

### Condiciones: `when` y rutas de `switch`

Declarativas, evaluadas en el host (nada de JS del usuario):

```json
{ "and": [ { "gte": ["$.cliente.pedidos", 10] }, { "eq": ["$.cliente.pais", "ES"] } ] }
```

Operadores: `eq`, `ne`, `gt`/`gte`/`lt`/`lte`, `exists`, `contains`;
combinadores `and`/`or`/`not`. Referencia no resuelta o tipos incomparables ⇒
fallo determinista, nunca una ruta elegida en silencio.

Para bifurcar la cadena, el bloque built-in **`switch`** declara rutas
nombradas a instancias posteriores y un `default`; las posiciones no tomadas
quedan como pasos `skipped` (visibles en el trace):

```json
{ "id": "b-switch", "name": "enrutador", "blockId": "<switch>", "blockVersionId": "<v>",
  "input": {}, "config": {
    "routes": [
      { "when": { "gte": ["$.trigger.body.total", 10] }, "to": "b-vip" },
      { "when": { "eq": ["$.cliente.pais", "ES"] }, "to": "b-iberia" }
    ],
    "default": "b-normal"
  } }
```

### Al terminar: `emit`, `webhook`, `mask`

| Sección | Forma | Efecto |
|---|---|---|
| `emit` | `{"onSuccess": ["cliente.creado"], "onFailure": ["cliente.fallo"]}` | publica eventos al completar — encadena otros puentes por topic |
| `webhook` | `{"url": "https://tuservidor/hook"}` | POST firmado con `X-Integraciones-Signature` (HMAC-SHA256), `X-Integraciones-Timestamp` y `X-Integraciones-Delivery`, con reintentos |
| `mask` | `["$.trigger.body.tarjeta", "$.creaOrden.rest.tarjeta"]` | `***` irreversible ANTES de persistir (trace, listado y webhook ya enmascarados) |

## Ensayar, validar y publicar

1. **Dry-run** (consola: *Ensayar sin efectos* / `POST …/dry-run`): corre la cadena con datos de prueba sin persistir filas ni disparar emit/webhook.
2. **Validar** (`POST …/validate`): refs imposibles, rutas hacia atrás, schemas — errores en línea sobre la instancia culpable.
3. **Publicar** (`POST …/versions`): congela la definición como versión numerada. Nada se ejecuta sin versión publicada.
4. **Versiones** (panel del editor): diff lado a lado (versión↔versión o borrador↔versión), adoptar la publicada al borrador y **rollback** (publica el contenido antiguo como versión NUEVA — el pasado jamás se reescribe).

## Invocar el puente

| Modo | Cómo | Respuesta |
|---|---|---|
| Síncrono | `POST /x/{slug}` con body | `200` con el sobre del resultado (`{"success":true,"data":…}`) |
| Asíncrono | mismo POST + `Prefer: respond-async` | `202` + `Location: /x/{slug}/executions/{id}` — polling hasta `{"status":"…","data":…}` |
| Cron/evento | automático según `trigger` | el resultado se observa en el trace |

El fallo de un bloque no tira el HTTP: la respuesta lleva el error
estructurado (`BLOCK_ERROR`, `TIMEOUT`, `VALIDATION_FAILED`…) y el trace
señala el paso exacto — [leer un trace](guia-consola.md#leer-una-ejecución-el-trace).

## Checklist antes de publicar

- [ ] Toda referencia `$.` apunta a un bloque ANTERIOR o a `trigger`/`now`
- [ ] `inputSchema` valida el body que esperas (si el trigger es http)
- [ ] `when`/`switch` cubren el caso "no encaja nada" (el `default` existe si hace falta)
- [ ] `POST …/validate` responde `"valid": true`
- [ ] Dry-run con mocks verificado (sin filas ni webhook)
- [ ] Datos sensibles bajo `mask` si van a aparecer en el trace
- [ ] `emit`/`webhook` solo si otro sistema debe enterarse del resultado

## Ver también

- [Guía: crear y publicar bloques](guia-bloques.md) — las piezas que esta cadena ensambla
- [Guía de la consola](guia-consola.md) — el editor, el trace y las versiones en pantalla
- [Errores comunes](errores-comunes.md) — diagnóstico rápido de 401/403/422 y códigos de ejecución
- Contrato completo: `/docs` (Scalar) o `openapi.json`
