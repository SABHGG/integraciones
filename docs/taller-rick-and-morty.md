# Taller: tu primer flujo completo — recopilar personajes de Rick and Morty

Recorrido guiado por el ciclo completo de la plataforma usando un caso real y
sin claves: crear **bloques** (uno built-in `http` y uno `custom_logic`),
configurar el **puente** que los encadena, ensayar, publicar, invocar y leer
el **trace**. Al terminar tendrás el puente `personajes-rm`: le pasas un
nombre y devuelve el personaje encontrado con su ficha de detalle.

Casi todo es operable desde la consola o por API (`/api/v1`) — cada paso
muestra las **dos rutas**; la validación siempre es del servidor. Donde la
consola no llega (los schemas y metadatos `ui` del bloque solo se publican
por API), tienes el curl exacto listo para copiar. Tiempo estimado:
30–45 minutos. Para los detalles finos de cada pieza, este taller enlaza a
las guías de referencia en lugar de repetirlas.

## Qué vas a construir

```
POST /x/personajes-rm  {"nombre": "rick"}
        │
        ▼
trigger http ── inputSchema valida el body ANTES de tocar bloques
        │
b1  traePersonajes     (http-get)          GET rickandmortyapi.com/api/character/?name={nombre}
        │  $.traePersonajes.results / .info.count / .info.pages
b2  resumePersonajes   (custom_logic v1)   resume → { total, paginas, primerId, resumen[] }
        │  $.resumePersonajes.primerId
b3  traeDetalle        (http-get)          GET rickandmortyapi.com/api/character/{primerId}
        │
        ▼
200  {"success": true, "data": { … }}
```

Aquí se practican: tipos de bloque, versiones inmutables, referencias `$.`,
interpolación en `config`, `inputSchema`, dry-run, validar → publicar,
invocación síncrona y asíncrona, trace y evolución a una v2 sin romper nada.

## Requisitos previos

- `./scripts/dev.sh` corriendo: API en `http://127.0.0.1:8080`, consola en
  `/console`. El seed deja la key de demo (`demo-key-1`) y los bloques
  built-in `http-get` / `http-post` publicados.
- Rol `editor` o superior en la consola (una API key actúa como `owner`).
- Para los ejemplos por API: `K='Authorization: Bearer demo-key-1'`.

## Paso 0 · Reconocer la API externa

La [API de Rick and Morty](https://rickandmortyapi.com) es pública: **sin
autenticación ni claves**. Es el caso simple; el paso opcional de
credenciales (ejercicio 5) muestra dónde iría la clave si la hubiera.

| Endpoint | Devuelve |
|---|---|
| `GET /api/character` | lista paginada de personajes (20 por página) |
| `GET /api/character/{id}` | ficha completa de un personaje |
| `GET /api/character/?name=rick` | búsqueda por nombre (misma forma que la lista) |
| `GET /api/location`, `GET /api/episode` | mismos patrones para otros dominios |

La respuesta de búsqueda tiene dos partes — pruébala en el navegador:

```json
{
  "info":    { "count": 107, "pages": 6, "next": "…?page=2&name=rick", "prev": null },
  "results": [ { "id": 1, "name": "Rick Sanchez", "status": "Alive",
                 "species": "Human", "gender": "Male",
                 "origin": { "name": "Earth (C-137)", "url": "…" },
                 "location": { "name": "Citadel of Ricks", "url": "…" },
                 "image": "…", "episode": [ "…", "…" ], "url": "…", "created": "…" } ]
}
```

> **Ojo**: si el nombre no existe, la API responde **404** con
> `{"error": "Character not found"}`. El paso `http` fallará alto y claro y
> el trace señalará `b1` — es un comportamiento deseable, no un accidente
> (ver [Si algo falla](#si-algo-falla)).

## Paso 1 · Los bloques

### 1a · El bloque built-in `http` (sin código)

El seed ya deja `http-get` publicado en tu biblioteca. En una instalación
limpia se crea con `POST /api/v1/blocks` (`blockType: "http"`) y se publica
una versión sin código (`{}`): no hay nada que versionar tú, su
comportamiento vive en la `config` de cada instancia del puente (paso 3).

En la consola: **Bloques** → localiza `http-get` → abre su detalle para
copiar el `id` del bloque y el `id` de su versión publicada (los necesitas
en el paso 3). La consola no crea bloques `http` (solo `custom_logic`): si
no está en tu biblioteca, créalo por API como se describe arriba.

### 1b · Crear el bloque `custom_logic`

El segundo bloque transforma los datos recopilados en un resumen.

**En la consola**: **Bloques → Nuevo bloque custom_logic** — formulario con
Slug, Nombre y Descripción → botón **Crear bloque**. La propia ficha avisa
de que el bloque se crea *sin código*: la v1 se publica después en su página
(paso 1c). Como la consola no pide categoría, si la quieres fijar (`transform`)
usarás la ruta por API:

| Campo | Valor |
|---|---|
| slug | `resume-personajes` |
| nombre | `Resume personajes` |
| categoría (solo API) | `transform` |
| descripción | `Resume info+results de la API de Rick and Morty en total, páginas y ficha del primer resultado` |

Por API:

```bash
curl -s -X POST localhost:8080/api/v1/blocks -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "resume-personajes", "name": "Resume personajes",
       "blockType": "custom_logic", "category": "transform",
       "description": "Resume info+results de la API de Rick and Morty"}'
```

Guarda el `id` de la respuesta: es el `blockId` de las instancias del paso 3.

### 1c · El código y la versión 1

El contrato es único: `export default (input, ctx) => salida` — el bloque ve
**solo su `input`** (el mapeo de la instancia, no el contexto completo) y su
salida debe cumplir el `outputSchema`. Puedes escribir TypeScript: se
transpila al publicar y los errores de transpile señalan la línea en el
editor de la consola.

```js
export default (input, ctx) => {
  const resultados = input.resultados ?? [];
  if (resultados.length === 0) {
    throw new Error("la búsqueda no devolvió personajes");
  }

  const limite = input.limite ?? 3;
  const resumen = resultados.slice(0, limite).map(p => ({
    id: p.id,
    nombre: p.name,
    estado: p.status,
    especie: p.species,
    origen: p.origin && p.origin.name
  }));

  ctx.log("info", "recopilados " + resultados.length +
                   " personajes de " + input.total + " (mostrando " + resumen.length + ")");

  return {
    total: input.total ?? resultados.length,
    paginas: input.paginas ?? 1,
    primerId: resumen[0].id,
    resumen
  };
};
```

Fíjate en las decisiones: falla alto y claro si no hay resultados (nunca
devuelve `{}` en silencio), usa `ctx.log` (cada línea se verá en el trace) y
no toca secretos (no tiene).

Los **schemas** del mismo envío de publicación — honestos: describen
exactamente lo que el código devuelve:

```json
{
  "inputSchema":  { "type": "object", "required": ["resultados"] },
  "outputSchema": { "type": "object", "required": ["total", "paginas", "primerId", "resumen"] }
}
```

Y los metadatos **`ui`**: quien configure la instancia en un puente verá un
formulario generado en lugar de JSON libre:

```json
{
  "ui": {
    "title": "Resumen de personajes",
    "order": ["limite"],
    "widget": { "limite": "number" },
    "help":   { "limite": "Cuántos personajes incluir en el resumen (por defecto 3)" }
  }
}
```

**Publica la v1** — dos rutas que se complementan:

**En la consola** — *Bloques → resume-personajes*: pega el código en
«Código (publicar nueva versión)» y pulsa **Publicar versión**; los errores
de transpile aparecen con su línea sobre el editor. Ojo: ese botón envía
**solo el `code`** — los schemas y los metadatos `ui` no se editan desde la
consola.

**Por API** — el envío completo (código + schemas + `ui` en la misma
llamada), con el `code` como string de una línea:

```bash
curl -s -X POST "localhost:8080/api/v1/blocks/$BLOQUE/versions" -H "$K" \
  -H 'content-type: application/json' -d '{
    "code": "export default (input, ctx) => { const resultados = input.resultados ?? []; if (resultados.length === 0) { throw new Error(\"la búsqueda no devolvió personajes\"); } const limite = input.limite ?? 3; const resumen = resultados.slice(0, limite).map(p => ({ id: p.id, nombre: p.name, estado: p.status, especie: p.species, origen: p.origin && p.origin.name })); ctx.log(\"info\", \"recopilados \" + resultados.length + \" personajes de \" + input.total + \" (mostrando \" + resumen.length + \")\"); return { total: input.total ?? resultados.length, paginas: input.paginas ?? 1, primerId: resumen[0].id, resumen }; }",
    "inputSchema":  { "type": "object", "required": ["resultados"] },
    "outputSchema": { "type": "object", "required": ["total", "paginas", "primerId", "resumen"] },
    "ui": { "title": "Resumen de personajes", "order": ["limite"],
            "widget": { "limite": "number" },
            "help":   { "limite": "Cuántos personajes incluir en el resumen (por defecto 3)" } }
  }'
```

La publicación valida transpile + schemas al instante y congela la **v1
inmutable**: guarda el `id` de esta versión (será el `blockVersionId`). Las
ediciones futuras publican v2, v3… sin tocar jamás la v1 — y las ejecuciones
en vuelo terminan con la versión con la que empezaron.

## Paso 2 · Crear el puente

**En la consola**: **Puentes → Nuevo puente** — pide slug y nombre, crea el
borrador vacío y te abre directamente su editor (disparador, cadena y
publicación se hacen ahí). **Por API**:

```bash
curl -s -X POST localhost:8080/api/v1/bridges -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "personajes-rm", "name": "Recopila personajes de Rick and Morty"}'
```

El `slug` decide la ruta de invocación: `POST /x/personajes-rm`. Guarda el
`id` del puente para el borrador.

## Paso 3 · Configurar el disparador y la cadena

Ahora el **borrador**: `trigger` + cadena de instancias. Cada instancia
referencia el `blockId` y el `blockVersionId` **exactos** del paso 1.

El `inputSchema` del puente valida el body en el plano de invocación: si
falta `nombre`, la ejecución falla con `VALIDATION_FAILED` **antes** de
tocar ningún bloque.

**En la consola** — *Puentes → personajes-rm (editor)*:

1. **Trigger**: en su campo «Config (JSON)» pega el disparador —
   `{ "type": "http", "method": "POST", "path": "/x/personajes-rm" }`.
2. **Input schema**: pega `{ "type": "object", "required": ["nombre"] }`.
3. **Cadena → Añadir instancia**: pulsa `+ http-get`, luego
   `+ resume-personajes` y otra vez `+ http-get` — se añaden al final en ese
   orden y ya quedan enlazadas a la **versión vigente** de cada bloque.
4. **Renombra** cada instancia (el nombre inicial es el slug del bloque):
   `traePersonajes`, `resumePersonajes`, `traeDetalle` — las refs `$.` usan
   ese name; con ↑/↓ reordenas si te equivocas. **No te saltes este paso**:
   los mapeos de este taller apuntan a esos nombres exactos, y sin renombrar
   la validación falla con «apunta al bloque …, que no existe» y, con dos
   `http-get` en la cadena, con «el nombre … ya lo usa».
5. **b1 y b3** (bloques `http`, sin `ui` → campo «Config (JSON)»): pega su
   `config` del JSON de abajo; **b2**: pega su mapeo en «input (refs $.
   hacia bloques anteriores, JSON)». Rellena `timeoutMs` en cada instancia.
6. **Guardar borrador** → **Validar** (los errores se pintan sobre la
   instancia culpable).

El mismo borrador, por API — un `PATCH` con `trigger` + cadena completa:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE" -H "$K" \
  -H 'content-type: application/json' -d '{
  "trigger": { "type": "http", "method": "POST", "path": "/x/personajes-rm" },
  "inputSchema": { "type": "object", "required": ["nombre"] },
  "blocks": [
    {
      "id": "b1", "name": "traePersonajes",
      "blockId": "<id de http-get>", "blockVersionId": "<versión publicada de http-get>",
      "input": {},
      "config": {
        "url": "https://rickandmortyapi.com/api/character/?name={$.trigger.body.nombre}",
        "method": "GET"
      },
      "timeoutMs": 8000
    },
    {
      "id": "b2", "name": "resumePersonajes",
      "blockId": "<id de resume-personajes>", "blockVersionId": "<v1 publicada>",
      "input": {
        "resultados": "$.traePersonajes.results",
        "total":      "$.traePersonajes.info.count",
        "paginas":    "$.traePersonajes.info.pages",
        "limite":     3
      },
      "timeoutMs": 3000
    },
    {
      "id": "b3", "name": "traeDetalle",
      "blockId": "<id de http-get>", "blockVersionId": "<versión publicada de http-get>",
      "input": {},
      "config": {
        "url": "https://rickandmortyapi.com/api/character/{$.resumePersonajes.primerId}",
        "method": "GET"
      },
      "timeoutMs": 8000
    }
  ]
}'
```

Lo que acabas de practicar:

| Pieza | Qué demuestra |
|---|---|
| `$.trigger.body.nombre` | el body del disparo llega al `config` de b1 |
| `{$.…}` dentro de la URL | interpolación de la `config` del built-in `http` |
| `$.traePersonajes.results` | la salida de un bloque **anterior**, por su `name` |
| `limite: 3` | un valor literal junto a las referencias (en la consola va en el textarea de `input`) |
| `$.resumePersonajes.primerId` | encadenar un segundo `http` con dato computado por `custom_logic` |
| `timeoutMs` explícitos | cada instancia con su techo — en el editor el campo avisa: «vacío = 5 s» |

> Si usas nombres con espacios (`"Rick Sanchez"`), comprueba que tu
> despliegue los codifique en la URL; para el taller, nombres de una palabra
> (`rick`, `morty`, `summer`) van seguros.

Una referencia que no resuelve es **fail-fast**: `REF_UNRESOLVED` en el
paso exacto, nunca una cadena silenciosa de `undefined`.

## Paso 4 · Ensayar, validar, publicar

En orden — nada se ejecuta de verdad sin versión publicada:

1. **Dry-run** — consola: panel **«Ensayo sin efectos»** del editor →
   «Datos de prueba para `$.trigger` (JSON)» con `{ "nombre": "morty" }` →
   botón **Ensayar**: verás el estado, cada paso y el output **sin**
   persistir ejecuciones ni disparar efectos (opcionalmente con `mocks` por
   instancia para fijar salidas — contrato exacto en `/docs`). API:
   `POST …/dry-run`.
2. **Validar** — consola: botón **Validar**; los errores aparecen sobre la
   instancia culpable. API: `POST …/validate` — refs imposibles, rutas hacia
   atrás, schemas; debe responder `"valid": true`.
3. **Publicar** — consola: botón **Publicar versión**. API:
   `POST …/versions`. Congela la **v1 del puente**: desde este momento las
   invocaciones corren contra ese snapshot exacto; editar el borrador
   después no las afecta.

El panel de **versiones** del editor muestra el diff lado a lado
(borrador↔versión, versión↔versión) — lo usarás en el paso 6.

## Paso 5 · Invocar y leer el trace

La consola **no dispara ejecuciones reales**: el disparo es siempre HTTP
(o cron/evento) — desde el editor solo ensayas sin efectos. Dispara por
HTTP y obsérvalo en la consola:

**Síncrono** — la respuesta lleva el sobre con los datos recopilados:

```bash
curl -s -X POST localhost:8080/x/personajes-rm -H 'content-type: application/json' \
  -d '{"nombre": "rick"}'
```

```json
{ "success": true,
  "data": { "traePersonajes": { "info": { "count": 107, "pages": 6, "…": "…" },
                             "results": [ "…20 personajes…" ] },
         "resumePersonajes": { "total": 107, "paginas": 6, "primerId": 1,
                            "resumen": [ { "id": 1, "nombre": "Rick Sanchez",
                                           "estado": "Alive", "especie": "Human",
                                           "origen": "Earth (C-137)" }, "…" ] },
         "traeDetalle": { "id": 1, "name": "Rick Sanchez", "status": "Alive",
                       "species": "Human", "origin": { "name": "Earth (C-137)" }, "…" } } }
```

**Asíncrono** — mismo POST con `Prefer: respond-async`: responde `202` con
`Location: /x/personajes-rm/executions/{id}` para consultar por polling.

**En la consola**: **Ejecuciones** → cada fila dibuja su *rastro en
miniatura* (un segmento por paso, proporcionado a su duración, coloreado por
estado) → abre el detalle para ver el trace paso a paso: input y output de
cada instancia, la línea `recopilados 20 personajes de 107 (mostrando 3)` de
`ctx.log`, quién disparó y duraciones reales.

## Paso 6 · Evolucionar sin romper nada

Practica la regla de oro — el pasado jamás se reescribe:

1. Edita `resume-personajes` (por ejemplo, añade `imagen: p.image` al
   resumen) y **publica la v2** del bloque. La v1 queda intacta.
2. En el borrador del puente, mueve `b2` a la v2 y **publica la v2 del
   puente**. En la consola lo más cómodo es quitar la instancia y pulsar de
   nuevo `+ resume-personajes` en «Añadir instancia» (enlaza siempre a la
   versión vigente): repon su `name`, su `input` y su `timeoutMs`. Por API:
   cambia el `blockVersionId` en el `PATCH` del borrador.
3. Vuelve a invocar: la nueva ejecución corre contra la v2. Cualquier
   ejecución antigua conserva su trace contra la v1.
4. En el panel de versiones del puente, prueba el **diff** entre v1 y v2 y
   el **rollback** (re-publica la v1 como versión *nueva*).

## Ejercicios propuestos

1. **Paginación**: `info.next` trae la página siguiente. Escribe un bloque
   `custom_logic` que use `ctx.http.request` (la única red del sandbox)
   para recopilar dos páginas y fusionar los resultados.
2. **Condiciones**: añade a `b3` un `when` (por ejemplo
   `{ "gte": ["$.resumePersonajes.total", 10] }`) y comprueba en el trace
   el estado `skipped` — el paso no se ejecutó, no es un error.
3. **Recopilación programada**: duplica el puente con trigger
   `{"type": "cron", "schedule": "0 */1 * * *"}` — cada hora recopila sin
   que nadie dispare nada (quién disparó: `system`).
4. **Encadenar puentes**: añade `emit: {"onSuccess": ["personaje.recopilado"]}`
   y crea un segundo puente suscrito a ese topic con trigger `event`.
5. **Credenciales** (si la API pidiera clave, hoy no): guarda el secreto en
   la bóveda (**Credenciales**, write-only) y úsalo como
   `{$credential:rm-key}` en los headers del `http`, o `ctx.credential("rm-key")`
   en el `custom_logic` — jamás aparecerá en el trace.
6. **Webhook de entrega**: añade `webhook: {"url": "…"}` y observa el POST
   firmado (HMAC-SHA256) con reintentos.

## Si algo falla

| Síntoma | Código / estado | Causa probable | Arreglo |
|---|---|---|---|
| La invocación ni empieza | `VALIDATION_FAILED` | falta `nombre` en el body | el `inputSchema` está haciendo su trabajo |
| `b1` falla con 404 de la API | `BLOCK_FAILED` | no existe ese personaje | la API respondió 404: el trace señala `b1` — prueba con otro nombre |
| `b2` o `b3` fallan con ref | `REF_UNRESOLVED` | `name` mal escrita o salida inesperada | corrige la referencia hacia el `name` real del bloque anterior |
| La ejecución se corta | `TIMEOUT` | la API externa tardó más que `timeoutMs` | sube el `timeoutMs` de la instancia |
| No deja publicar el bloque | error de transpile/schema en el editor | el código o el `outputSchema` no coinciden | el validador señala la línea; ajusta schema o código |
| 422 al guardar el borrador | Problem Details (RFC 9457) | JSON inválido o campo desconocido | el `detail` del problema dice qué campo |

**Y si al guardar o validar el borrador responde 422** («el borrador no pasa
la validación»), los tres casos típicos de este taller:

- `la referencia "$.…" apunta al bloque "traePersonajes", que no existe en
  la cadena` → las instancias siguen llamándose como su slug: renómbralas
  (paso 3.4) o cambia las refs a los `name` reales.
- `el nombre "http-get" ya lo usa blocks[0]` → añadiste el mismo bloque dos
  veces sin renombrar: cada instancia necesita un `name` **único**.
- `referencia inválida … interpolación sin cerrar` → falta el `}` de cierre
  en la URL: debe terminar exactamente en `{$.trigger.body.nombre}`.

Diagnóstico ampliado: [errores-comunes.md](errores-comunes.md).

## Checklist del taller

- [ ] `http-get` localizado en la biblioteca (ids de bloque y versión copiados)
- [ ] `resume-personajes` v1 publicada con código, schemas honestos y `ui`
- [ ] Puente `personajes-rm` creado; borrador con trigger + `inputSchema` + 3 instancias
- [ ] Dry-run verificado con `{"nombre": "morty"}` (sin filas ni efectos)
- [ ] `POST …/validate` → `"valid": true`; **v1 del puente publicada**
- [ ] Invocación síncrona `200` con `total: 107` y `resumen` de 3 personajes
- [ ] Trace leído en la consola (estados, IO por paso y la línea de `ctx.log`)
- [ ] V2 del bloque + v2 del puente publicadas; diff y rollback probados

## Ver también

- [Guía: crear y publicar bloques](guia-bloques.md) — contrato del código, sandbox `ctx.*`, metadatos `ui`
- [Guía: crear y publicar puentes](guia-puentes.md) — disparadores, refs `$.`, `when`/`switch`, emit/webhook/mask
- [Guía de la consola](guia-consola.md) — páginas, roles, rastro visual y trace en pantalla
- [Errores comunes](errores-comunes.md) — diagnóstico rápido de HTTP y códigos de ejecución
- Contrato completo de la API: `/docs` (Scalar) u `openapi.json`
