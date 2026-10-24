# Taller 3 · Integración con credenciales: hablar a una API autenticada

Caso real: consumir una API que **exige autenticación Bearer** —
`httpbin.org/bearer` responde `401` sin token y `200` con él (y, como buen
eco de prueba, devuelve el token en la respuesta, que nos servirá para
practicar `mask`). Al terminar sabrás guardar secretos en la bóveda,
usarlos desde bloques `http` y `custom_logic`, ocultarlos del trace y
**rotarlos sin tocar ningún puente**.

Prerrequisito recomendado: [Taller 1](taller-rick-and-morty.md).

## Qué vas a construir

```
POST /x/consulta-token  {}
        │
b1  consultaToken   (http-get)   GET https://httpbin.org/bearer
    config.headers: { "Authorization": "Bearer {$credential:httpbin-token}" }
        │
        ▼
200 {"authenticated": true, …}   — y con mask, el token jamás se persiste
```

El secreto vive **solo en la bóveda**: el puente referencia su *nombre*, no
su valor. Ni el borrador, ni el trace, ni las ejecuciones lo contienen.

## Paso 0 · Reconocer la API y su 401

Abre `https://httpbin.org/bearer` en el navegador — verás el rechazo:

```json
{ "message": "Requires authentication" }
```

Con cabecera `Authorization: Bearer <cualquier-token>` responde `200`:

```json
{ "authenticated": true, "token": "<tu-token>" }
```

Fíjate en el eco: la API **devuelve el token que le enviaste**. Es perfecto
para el taller, porque el output del bloque se persiste en el trace — y no
queremos secretos persistidos. Llegaremos a eso en el paso 5.

## Paso 1 · Guardar el secreto en la bóveda

La bóveda es **write-only**: puedes crear y rotar, pero el valor jamás se
vuelve a leer — ni por la consola ni por la API. El historial muestra
versiones (ids y fechas), nunca valores.

**En la consola**: *Credenciales → nueva credencial* — nombre
`httpbin-token`, secreto `mi-token-secreto-2026`. Verás que el formulario no
ofrece forma de verlo después: solo rotarlo. (El seed ya dejó una credencial
de ejemplo, `pago-api` — no la toques, pertenece al puente demo.)

**Por API**:

```bash
curl -s -X POST localhost:8080/api/v1/credentials -H "$K" -H 'content-type: application/json' \
  -d '{"name": "httpbin-token", "secret": "mi-token-secreto-2026"}'
```

> **Aviso de desarrollo**: si `VAULT_KEY` no está definida en tu `.env`, el
> servidor cifra con una clave efímera por proceso — al reiniciar, la
> credencial dejará de descifrarse. En dev, `dev.sh` ya lo gestiona; para un
> entorno que deba sobrevivir a reinicios, define `VAULT_KEY` (64 hex).

## Paso 2 · Crear el puente

**En la consola**: *Puentes → Nuevo puente* — slug `consulta-token`. **Por
API**:

```bash
curl -s -X POST localhost:8080/api/v1/bridges -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "consulta-token", "name": "Consulta autenticada de ejemplo"}'
```

## Paso 3 · La cadena: `{$credential:x}` en la config del `http`

El placeholder `{$credential:nombre}` puede aparecer en cualquier string de
la `config` de un built-in `http` — típicamente en `headers`. El host lo
sustituye por el valor descifrado **en memoria, justo antes de la llamada**;
el valor jamás entra al borrador ni al trace.

**En la consola** — *Puentes → consulta-token (editor)*:

1. **Trigger**: `{ "type": "http", "method": "POST", "path": "/x/consulta-token" }`.
2. **Cadena → Añadir instancia**: `+ http-get` → renómbrala `consultaToken`.
3. **b1** («Config (JSON)»):
   ```json
   {
     "url": "https://httpbin.org/bearer",
     "method": "GET",
     "headers": { "Authorization": "Bearer {$credential:httpbin-token}" }
   }
   ```
4. **Guardar borrador** → **Validar** → **Publicar versión**.

**Por API** — el mismo borrador de una pieza:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE" -H "$K" \
  -H 'content-type: application/json' -d '{
  "trigger": { "type": "http", "method": "POST", "path": "/x/consulta-token" },
  "blocks": [
    {
      "id": "b1", "name": "consultaToken",
      "blockId": "<id de http-get>", "blockVersionId": "<versión publicada de http-get>",
      "input": {},
      "config": {
        "url": "https://httpbin.org/bearer",
        "method": "GET",
        "headers": { "Authorization": "Bearer {$credential:httpbin-token}" }
      },
      "timeoutMs": 8000
    }
  ]
}'
```

## Paso 4 · Invocar y mirar el trace (con lupa)

```bash
curl -s -X POST localhost:8080/x/consulta-token -H 'content-type: application/json' -d '{}'
```

```json
{ "success": true, "data": { "consultaToken": { "authenticated": true,
                             "token": "mi-token-secreto-2026" } } }
```

Funciona. Pero abre la ejecución en *Ejecuciones* y mira el output de `b1`:
**el token está ahí**, persistido tal cual — no porque la plataforma lo
filtre, sino porque `httpbin` lo devuelve en su respuesta. Un trace acabó de
convertirse en un gestor de secretos muy malo. Vamos a arreglarlo.

## Paso 5 · `mask`: el secreto no vuelve a tocar el trace

`mask` es una lista de referencias `$.` del borrador que se reemplazan por
`***` **de forma irreversible y antes de persistir**: lo que ves en el
trace, el listado y el webhook ya viene enmascarado — no hay forma de
recuperar el valor original.

El enmascarado no se edita desde el editor de la consola; se añade al
borrador por API y se publica como versión nueva:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE" -H "$K" \
  -H 'content-type: application/json' \
  -d '{"mask": ["$.consultaToken.token"]}'
```

Después: **Guardar** (en consola, tras adoptar los cambios o re-PATCH),
**Validar** → **Publicar versión** (será la v2) → invoca de nuevo:

```json
{ "success": true, "data": { "consultaToken": { "authenticated": true,
                             "token": "***" } } }
```

El trace de la ejecución nueva ya no contiene el secreto — ni este ni
ningún resultado futuro. Comprueba en el panel de versiones el diff v1↔v2:
la única línea que cambió es `mask`.

## Paso 6 · Rotar sin tocar el puente

El pago de la bóveda llega cuando el token caduca o se filtra:

**En la consola**: *Credenciales → httpbin-token → Rotar* — escribe el nuevo
valor. El historial gana una versión (fecha e id, jamás valores). **Por
API**: el endpoint de rotación está documentado en `/docs` (Scalar).

Y ahora invoca de nuevo:

```bash
curl -s -X POST localhost:8080/x/consulta-token -H 'content-type: application/json' -d '{}'
```

`200` al primer intento. El puente sigue publicando la misma versión con la
misma referencia `{$credential:httpbin-token}` — la rotación es invisible
para quien construye puentes. Ningún puente editado, ninguna versión
publicada.

## Variante `custom_logic`: `ctx.credential`

Si el bloque `custom_logic` necesita el secreto (p. ej. para firmar una
petición a mano), lo obtiene con `ctx.credential("nombre")` — devuelve el
valor descifrado o vacío si no existe — y puede llamar a la red con
`ctx.http.request` (el sandbox resuelve `async/await`):

```js
export default async (input, ctx) => {
  const token = ctx.credential("httpbin-token");
  if (!token) {
    throw new Error("la credencial httpbin-token no está disponible");
  }

  const respuesta = await ctx.http.request({
    url: "https://httpbin.org/bearer",
    method: "GET",
    headers: { "Authorization": "Bearer " + token }
  });

  ctx.log("info", "autenticado: " + respuesta.authenticated);
  return { autenticado: respuesta.authenticated };
};
```

Regla de oro: el secreto se usa y se calla — nunca lo devuelvas en la salida
ni lo pases a `ctx.log`; y si la API lo va a hacer *eco* en su respuesta
(Como httpbin), añade `mask` sobre esa ruta.

## Ejercicios propuestos

1. **Clave en query string**: muchas APIs usan `?api_key=…`. Haz que la URL
   de la `config` sea
   `https://api.ejemplo.com/datos?api_key={$credential:httpbin-token}`.
2. **Dos credenciales en un puente**: encadena dos llamadas `http` que usen
   credenciales distintas (crea una segunda en la bóveda).
3. **Borrar la credencial**: *Credenciales → httpbin-token → Eliminar* (te
   exige teclear el nombre) e invoca: el fallo es alto y claro, no silencioso.
   Vuelve a crearla con el mismo nombre y todo vuelve a funcionar.
4. **Auditoría del historial**: rota dos veces y recorre el historial —
   comprueba que ni la consola ni la API exponen valores, solo fechas e ids.

## Si algo falla

| Síntoma | Causa probable | Arreglo |
|---|---|---|
| `b1` falla y la API respondió 401 | token mal escrito o credencial rotada a un valor inválido | rota de nuevo con el valor correcto — sin tocar el puente |
| Fallo resolviendo `{$credential:…}` | el nombre no existe en la bóveda (o cambió) | revísalo en *Credenciales*; el placeholder debe usar el **nombre** exacto |
| Tras reiniciar el servidor, la credencial no descifra | `VAULT_KEY` efímera (no definida) | define `VAULT_KEY` y vuelve a crear la credencial |
| El token sigue apareciendo en el trace | el `mask` quedó en el borrador sin publicar, o la ref no coincide | pubícate la versión nueva y revisa la ref: debe ser `$.<name>.<campo>` |
| 422 al guardar el borrador | JSON inválido o `name` duplicado | el `detail` del problema señala el campo (taller 1, «si algo falla») |

## Checklist del taller

- [ ] Credencial `httpbin-token` creada en la bóveda (y jamás vuelta a leer)
- [ ] Puente `consulta-token` publicado con `{$credential:httpbin-token}` en `headers`
- [ ] Eco del token detectado en el trace (paso 4) y corregido con `mask`
- [ ] v2 publicada con `mask: ["$.consultaToken.token"]`; trace mostrando `***`
- [ ] Rotación probada: nueva ejecución `200` sin editar el puente
- [ ] Variante `ctx.credential` probada en un bloque `custom_logic`

## Ver también

- [Taller 1: Rick and Morty](taller-rick-and-morty.md) — la base de todo
- [Taller 2: Recopilación programada](taller-cron-programado.md) — cron y webhook
- [Taller 4: Eventos y webhooks](taller-eventos-webhooks.md) — firma HMAC del webhook
- [Guía: crear y publicar bloques](guia-bloques.md) — sandbox `ctx.*` completo
- [Guía: crear y publicar puentes](guia-puentes.md) — `mask` y placeholders en detalle
- [Guía de la consola](guia-consola.md) — la bóveda write-only en pantalla
