# Taller 4 · Integración dirigida por eventos: emit, fan-out y webhooks firmados

Caso real: **desacoplar "recopilar" de "reaccionar"**. El puente que recopila
personajes (el del taller 1) no sabrá quién consume sus resultados: publica
el topic `personaje.recopilado` al terminar, y **cualquier puente suscrito**
reacciona por su cuenta. Practicarás `emit`, el trigger `event` con fan-out,
y la entrega por webhook firmado — incluida la **verificación HMAC-SHA256 en
el receptor**, tal y como la firma la plataforma.

Prerrequisito: [Taller 1](taller-rick-and-morty.md) (reutiliza su puente
`personajes-rm` publicado).

## Qué vas a construir

```
                        publica manualmente
  POST /api/v1/events/personaje.recopilado  ────────────────┐
                                                            ▼ fan-out
POST /x/personajes-rm ──► puente personajes-rm ──emit──► topic `personaje.recopilado`
        (taller 1)                                          │
                                                            ▼
                     puente reacciona-personaje  (trigger event)
                        b1 arma el mensaje ──► webhook firmado ──► webhook.site
                                                                  + verificador HMAC
```

La gracia del modelo: el emisor no conoce a los receptores (ni al revés).
Hoy hay un suscriptor; mañana añades cinco sin tocar el emisor.

## Paso 0 · El receptor desechable

Abre [webhook.site](https://webhook.site) y copia tu URL única
(`https://webhook.site/<uuid>`). Verás ahí llegar, en vivo, cada entrega con
sus cabeceras — no hace falta montar un servidor para este taller.

## Paso 1 · El emisor: publicar un topic al completar

El puente `personajes-rm` del taller 1 aprende a anunciar que terminó. La
sección `emit` declara qué topics publica — al completar con éxito
(`onSuccess`) o al fallar (`onFailure`). Como el resto de secciones del
borrador que el editor no expone, va por API:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE_RM" -H "$K" \
  -H 'content-type: application/json' \
  -d '{"emit": {"onSuccess": ["personaje.recopilado"]}}'
```

Después: **Validar** → **Publicar versión** (los cambios solo viven cuando
publicas — las ejecuciones corren contra versiones congeladas, nunca contra
el borrador).

## Paso 2 · El suscriptor: trigger `event`

**En la consola**: *Puentes → Nuevo puente* — slug `reacciona-personaje`.
En su editor:

1. **Trigger** (campo «Config (JSON)»):
   `{ "type": "event", "topic": "personaje.recopilado" }`.
2. **Cadena → Añadir instancia**: `+ enriquece-cliente` sirve de base, pero
   para el taller es más claro un bloque propio: crea en *Bloques* el
   `custom_logic` `arma-aviso` con este código y publica su v1 (consola:
   editor de código; schemas y `ui`: por API, como en los talleres
   anteriores):
   ```js
   export default (input, ctx) => {
     const p = input.payload ?? {};
     const personaje = p.personaje ?? p.nombre ?? "?";
     ctx.log("info", "reacción ante: " + personaje);
     return { personaje, mensaje: "Nuevo personaje recopilado: " + personaje };
   };
   ```
   ```json
   { "inputSchema":  { "type": "object", "required": ["payload"] },
     "outputSchema": { "type": "object", "required": ["personaje", "mensaje"] } }
   ```
3. Añádelo a la cadena y renómbralo `armaAviso`. Su **input** (textarea
   JSON): en un trigger `event`, `$.trigger` es el **sobre** del evento —
   `{topic, eventId, payload, receivedAt}` — y el payload que publicaste va
   dentro, en `$.trigger.payload`:
   ```json
   { "payload": "$.trigger.payload" }
   ```
4. **Guardar borrador** → **Validar** → **Publicar versión**.

**Por API** — el borrador completo:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE_RX" -H "$K" \
  -H 'content-type: application/json' -d '{
  "trigger": { "type": "event", "topic": "personaje.recopilado" },
  "blocks": [
    {
      "id": "b1", "name": "armaAviso",
      "blockId": "<id de arma-aviso>", "blockVersionId": "<v1 publicada>",
      "input": { "payload": "$.trigger" },
      "timeoutMs": 3000
    }
  ],
  "webhook": { "url": "https://webhook.site/<tu-uuid>" }
}'
```

## Paso 3 · Publicar un evento a mano (y entender el fan-out)

Sin depender del emisor, publica el topic directamente — así lo haría
cualquier sistema externo:

```bash
curl -s -X POST localhost:8080/api/v1/events/personaje.recopilado -H "$K" \
  -H 'content-type: application/json' \
  -d '{"personaje": "rick", "total": 107}'
```

```json
{ "topic": "personaje.recopilado", "eventId": "…", "executions": 1 }
```

`executions` es cuántos suscriptores se encolaron. Tres cosas que acaba de
enseñarte la plataforma:

- **Fan-out transaccional**: se encola una ejecución por cada puente
  suscrito activo (su última versión publicada sobre ese topic).
- **`202` aunque no haya nadie escuchando**: el evento no se retiene — no
  hay cola de eventos pendientes ni log de eventos. Si publicas mal escrito
  el topic, no falla: simplemente nadie reacciona.
- **Auditoría**: en *Ejecuciones*, la ejecución del suscriptor muestra como
  disparador a quien publicó el evento (tu usuario o tu API key) — publicar
  eventos requiere rol `runner`.

En webhook.site verás la entrega del resultado de `reacciona-personaje`.
Pero esta entrega viene **firmada** — vamos a comprobarlo de verdad.

## Paso 4 · La entrega firmada y su verificación en el receptor

Cada entrega del webhook lleva tres cabeceras:

| Cabecera | Valor |
|---|---|
| `X-Integraciones-Signature` | `sha256=<hex>` — HMAC-SHA256 del **body crudo** con el secreto del puente |
| `X-Integraciones-Timestamp` | unix seconds del intento |
| `X-Integraciones-Delivery` | id **estable** de la entrega: los reintentos de un mismo resultado comparten header y `executionId` — deduplicable |

El secreto es único por puente, estable entre versiones, y se recupera por
el plano admin (404 si el puente no tiene webhook publicado):

```bash
curl -s localhost:8080/api/v1/bridges/$PUENTE_RX/webhook-secret -H "$K"
```

Un receptor honesto no confía en el POST solo porque llegue: verifica la
firma. En Node.js — ojo al detalle clave, se firma el **body crudo**, no el
JSON re-serializado (re-serializar puede cambiar espaciado y rompería la
comparación):

```js
import { createHmac, timingSafeEqual } from "node:crypto";

export function verificarEntrega(rawBody, headers, secreto) {
  const esperada = "sha256=" +
    createHmac("sha256", secreto).update(rawBody).digest("hex");
  const recibida = headers["x-integraciones-signature"] ?? "";
  const ok = recibida.length === esperada.length &&
    timingSafeEqual(Buffer.from(recibida), Buffer.from(esperada));

  return {
    ok,
    delivery: headers["x-integraciones-delivery"], // deduplica aquí
    timestamp: headers["x-integraciones-timestamp"]
  };
}
```

Compara las cabeceras de webhook.site con tu cálculo: coinciden. Y el
`X-Integraciones-Delivery` es tu clave de idempotencia — si la plataforma
reintenta, es el mismo id: el receptor no debe procesar dos veces.

**Reintentos**: si tu receptor no responde `2xx`, la entrega reintenta con
backoff fijo — 30 s → 2 m → 10 m → 1 h → 6 h (5 intentos en total). Un
receptor caído un rato no pierde el resultado: lo recibe al volver.

## Paso 5 · El circuito completo: emit desde el emisor

Ahora el flujo de verdad, sin curl de eventos:

```bash
curl -s -X POST localhost:8080/x/personajes-rm \
  -H 'content-type: application/json' -d '{"nombre": "morty"}'
```

Al completar el puente emisor, `emit` publica `personaje.recopilado`, el
fan-out encola `reacciona-personaje`, y su resultado aterriza firmado en
webhook.site. Dos ejecuciones encadenadas sin que ningún puente conozca al
otro — en *Ejecuciones* las verás con sus disparadores respectivos (la tuya
y la del emisor de eventos).

## Ejercicios propuestos

1. **Fan-out de verdad**: duplica `reacciona-personaje` (cambia solo el
   mensaje del bloque) y vuelve a publicar el evento manual — `202` con dos
   ejecuciones encoladas. Añadir consumidores no tocó al emisor.
2. **`onFailure`**: añade `{"onFailure": ["personaje.fallo"]}` al emisor y
   suscríbete a ese topic con un puente de alerta.
3. **Enrutar por contenido**: en el suscriptor, añade un bloque built-in
   `switch` que derive a pasos distintos según `$.armaAviso.personaje`
   (repasa [`guia-puentes.md`](guia-puentes.md), rutas `routes` + `default`).
4. **Receptor caído**: apunta el webhook a una URL que responda 500 y
   observa en webhook.site (o en tus logs) los reintentos con su backoff.
5. **Deduplicación real**: guarda los `X-Integraciones-Delivery` procesados
   en tu receptor y demuestra que un reintento repetido no duplica efecto.

## Si algo falla

| Síntoma | Causa probable | Arreglo |
|---|---|---|
| `202` pero cero ejecuciones | topic mal escrito o suscriptor sin publicar | compara el `topic` del trigger con el publicado; **Publicar versión** |
| El suscriptor se dispara pero `b1` falla con ref | usaste `$.trigger.body.…` o `$.trigger.personaje` | el payload va en `$.trigger.payload`; `$.trigger` es el sobre `{topic, eventId, payload, receivedAt}` |
| Tu verificador dice firma inválida | verificaste el JSON re-serializado | firma y verifica el **body crudo**, bytes tal cual llegaron |
| El webhook no llega | URL sin https o receptor sin 2xx | reintentos 30 s → 2 m → 10 m → 1 h → 6 h; corrige el receptor |
| `emit` no publica nada | el `emit` quedó en el borrador sin publicar | los cambios solo valen publicados: **Publicar versión** |
| 404 en `webhook-secret` | el puente no tiene webhook configurado | añade `"webhook": {"url": "…"}` al borrador y publica |

## Checklist del taller

- [ ] `personajes-rm` publicando `personaje.recopilado` con `emit` (vN nueva)
- [ ] Bloque `arma-aviso` v1 publicado; puente `reacciona-personaje` publicado con trigger `event`
- [ ] Evento manual publicado por API: `202` y ejecución del suscriptor vista
- [ ] Entrega firmada recibida en webhook.site (Signature/Timestamp/Delivery)
- [ ] Secreto recuperado por `webhook-secret` y firma verificada con HMAC-SHA256
- [ ] Circuito completo probado: invocar emisor → suscriptor reacciona → webhook firmado
- [ ] Fan-out a dos suscriptores probado (ejercicio 1)

## Ver también

- [Taller 1: Rick and Morty](taller-rick-and-morty.md) — el puente emisor de este taller
- [Taller 2: Recopilación programada](taller-cron-programado.md) — cron + webhook, el otro disparo automático
- [Taller 3: API autenticada](taller-credenciales-api.md) — secretos y `mask`
- [Guía: crear y publicar puentes](guia-puentes.md) — `emit`, `webhook`, `mask` y `switch` en detalle
- [Errores comunes](errores-comunes.md) — diagnóstico rápido de webhooks y eventos
