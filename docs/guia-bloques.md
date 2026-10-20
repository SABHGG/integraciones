# Guía: crear y publicar bloques

Un **bloque** es la pieza reutilizable de la biblioteca: una unidad con `slug`,
metadatos y **versiones inmutables** — publicar una versión nueva nunca toca
las anteriores. Los puentes encadenan instancias de estos bloques; esta guía
cubre elegir el tipo, escribir el código, publicar versiones y lo que el
sandbox permite. Todo es operable desde la consola (**Bloques**) o por API
(`/api/v1`); la validación siempre es del servidor.

## Quick path

1. **Elige el tipo**: built-in `http` (ya existe, solo configúralo) o `custom_logic` (tu código).
2. **Crea el bloque**: consola → *Bloques → Nuevo bloque*, o `POST /api/v1/blocks`.
3. **Publica la v1**: escribe el código, define `inputSchema`/`outputSchema`/`ui` y publica — la consola muestra los errores de transpile con su línea.
4. **Úsalo en un puente**: → [Guía: crear y publicar puentes](guia-puentes.md).

## Elige el tipo

| | built-in `http` | `custom_logic` |
|---|---|---|
| **Quién ejecuta** | el host (sin código) | tu código en el sandbox QuickJS |
| **Para qué** | llamar una API: GET/POST con headers y body | transformar, firmar, decidir, orquestar llamadas |
| **Configuración** | `config` de la instancia (con interpolación) | `input` de la instancia + código versionado |
| **Secretos** | placeholders `{$credential:nombre}` | `ctx.credential("nombre")` |

Si el bloque solo "llama a un endpoint y devuelve el JSON", usa `http`. En
cuanto necesites computar, condicionar o combinar llamadas, `custom_logic`.

## Bloque built-in `http` (sin código)

El host lo ejecuta desde la `config` de la instancia en el puente. Soporta
interpolación con refs `{$.ruta}` y placeholders de credencial
`{$credential:nombre}` (el secreto solo vive en memoria del host; jamás entra
al trace):

```json
{
  "url": "https://api.proveedor.com/users/{$.trigger.body.userId}",
  "method": "GET",
  "headers": { "X-Api-Key": "{$credential:pago-api}" }
}
```

No hay nada que versionar tú: la biblioteca ya trae `http-get` y `http-post`
publicados. El `timeoutMs` se define **por instancia** en el puente.

## Bloque `custom_logic` (tu código)

### Crear

Consola → *Bloques → Nuevo bloque custom_logic* (slug, nombre, descripción), o
por API:

```bash
curl -X POST /api/v1/blocks -H "Authorization: Bearer $KEY" \
  -H 'content-type: application/json' \
  -d '{"slug": "enriquece-cliente", "name": "Enriquece cliente",
       "blockType": "custom_logic", "category": "transform",
       "description": "Pone el nombre en mayúsculas"}'
```

### El contrato del código

Un único `export default (input, ctx) => salida`:

- **`input`** es el mapeo `input` de la instancia ya resuelto — el bloque ve
  SOLO su entrada, no el contexto completo de la ejecución.
- **La salida debe cumplir el `outputSchema`**: se verifica estáticamente al
  publicar. Un schema honesto hoy evita puentes rotos mañana.
- **Un error de transpile (TS→JS con swc) bloquea la publicación** y el
  validador señala la línea en el editor de la consola.

Ejemplo completo — publicar la v1 con schemas y metadatos `ui`:

```bash
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

### Metadatos `ui` (formularios de la consola)

Opcionales; sin ellos la instancia se configura como JSON libre validado
contra `inputSchema`.

| Clave | Forma | Efecto |
|---|---|---|
| `title` | string | título del formulario |
| `order` | `["campo", …]` | orden de los campos |
| `widget` | `{campo: id}` | `text`, `url`, `number`, `textarea`, `password` (desconocido ⇒ texto) |
| `help` | `{campo: texto}` | ayuda bajo el campo |

### Versiones inmutables

Publicar una versión nueva (`POST /blocks/{id}/versions`) congela el código
vigente hasta entonces: `latestVersion` avanza y las ejecuciones en curso
terminan con la versión con la que empezaron. Las instancias de los puentes
fijan `blockVersionId` — actualizar un puente a la versión nueva es una
edición explícita tuya.

## El sandbox: qué tienes y qué no

QuickJS con límite de memoria y `timeoutMs` por instancia; **sin** red
directa, procesos ni módulos externos. Lo que sí tienes:

| API | Uso |
|---|---|
| `ctx.http.request({url, method, headers, body})` | la única red; `url` obligatorio; el host puede restringir dominios |
| `ctx.credential("nombre")` | valor descifrado de la bóveda; nunca entra al IO persistido |
| `ctx.log("info", "…")` | logs visibles en el trace |
| `ctx.crypto.sha256(s)` · `hmacSha256(clave, s)` · `base64Encode` · `base64Decode` | firma y codificación |
| `ctx.json.parse(s)` · `ctx.json.stringify(v)` | JSON seguro |
| `input` | la entrada resuelta (ver refs `$.` en la [guía de puentes](guia-puentes.md)) |

## Buenas prácticas

- **Un bloque, una responsabilidad**: "trae usuario" y "enriquece cliente" son dos bloques; el puente los compone.
- **`outputSchema` honesto**: es el contrato que los puentes de otros usarán con refs `$.`.
- **Secretos solo por `ctx.credential`**: nunca los devuelvas en la salida ni los loguees.
- **Determinista ante el fallo**: lanza (o deja que el schema falle) en vez de devolver silenciosamente `{}`.
- **`ctx.log` con criterio**: cada línea aparece en el trace de cada ejecución.

## Checklist antes de publicar

- [ ] El `outputSchema` coincide exactamente con lo que el código devuelve
- [ ] Probado el caso de entrada vacía o incompleta (¿falla alto y claro?)
- [ ] Ningún secreto en salidas, logs ni errores
- [ ] `timeoutMs` razonable pensado para la instancia (el default no es tuyos)
- [ ] La `ui` describe los campos que un humano configurará en el puente

## Ver también

- [Guía: crear y publicar puentes](guia-puentes.md) — encadena tus bloques detrás de un disparador
- [Guía de la consola](guia-consola.md) — dónde vive todo esto
- [Errores comunes](errores-comunes.md) — diagnóstico rápido de 401/403/422 y códigos de ejecución
- Contrato completo: `/docs` (Scalar) o `openapi.json`
