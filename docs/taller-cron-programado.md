# Taller 2 · Integración programada: vigilante meteorológico con cron

Caso real: **cada 15 minutos**, sin que nadie dispare nada, el puente consulta
la temperatura actual de Madrid en [Open-Meteo](https://open-meteo.com) (API
pública, sin clave), la interpreta con un bloque `custom_logic` y **entrega el
informe por webhook** a una URL tuya. Al terminar entenderás el trigger
`cron` completo: horario, timezone, qué pasa si el servidor duerme y por qué
no se invoca por `/x`.

Prerrequisito recomendado: [Taller 1](taller-rick-and-morty.md) — aquí se
asume que ya sabes crear bloques, renombrar instancias, ensayar y publicar.

## Qué vas a construir

```
scheduler (cada 15 min, Europe/Madrid)  ── disparo por `system`
        │
b1  traeClima        (http-get)   GET api.open-meteo.com/v1/forecast?…&current=…
        │  $.traeClima.current.temperature_2m / .wind_speed_10m / .time
b2  interpretaClima  (custom_logic v1)  →  { informe, nivel, temperatura, viento }
        │
        ▼
webhook ── POST firmado a tu URL (webhook.site para probar) + trace en la consola
```

## Paso 0 · Conocer la API externa

Open-Meteo es pública y sin autenticación. Abre en el navegador (respuesta
real, verificada en vivo):

```
https://api.open-meteo.com/v1/forecast?latitude=40.4168&longitude=-3.7038&current=temperature_2m,wind_speed_10m
```

```json
{
  "current_units": { "temperature_2m": "°C", "wind_speed_10m": "km/h", "…": "…" },
  "current": { "time": "2026-10-22T05:45", "interval": 900,
               "temperature_2m": 7.0, "wind_speed_10m": 3.9 }
}
```

La ruta que te interesa es `current` — fija, sin paginación ni filtros: el
caso ideal para un puente programado.

## Paso 1 · El bloque que interpreta

**En la consola**: *Bloques → Nuevo bloque custom_logic* — slug
`interpreta-clima`, nombre `Interpreta clima`, descripción `Clasifica la
temperatura actual y redacta un informe`. Publica en su editor el siguiente
código (recuerda: el botón **Publicar versión** de la consola envía solo el
`code`; los schemas y `ui` van en la llamada por API de abajo):

```js
export default (input, ctx) => {
  const t = input.temperatura;
  const nivel = t < 10 ? "frio" : t < 25 ? "templado" : "calor";

  ctx.log("info", "lectura de " + input.hora + ": " + t + " °C (" + nivel + ")");

  return {
    temperatura: t,
    viento: input.viento,
    nivel,
    informe: "En Madrid hay " + t + " °C (" + nivel +
             "), viento a " + input.viento + " km/h."
  };
};
```

**Por API** — la v1 completa (código + schemas + `ui` en la misma llamada):

```bash
curl -s -X POST "localhost:8080/api/v1/blocks/$BLOQUE/versions" -H "$K" \
  -H 'content-type: application/json' -d '{
    "code": "export default (input, ctx) => { const t = input.temperatura; const nivel = t < 10 ? \"frio\" : t < 25 ? \"templado\" : \"calor\"; ctx.log(\"info\", \"lectura de \" + input.hora + \": \" + t + \" °C (\" + nivel + \")\"); return { temperatura: t, viento: input.viento, nivel, informe: \"En Madrid hay \" + t + \" °C (\" + nivel + \"), viento a \" + input.viento + \" km/h.\" }; }",
    "inputSchema":  { "type": "object", "required": ["temperatura", "viento", "hora"] },
    "outputSchema": { "type": "object", "required": ["temperatura", "viento", "nivel", "informe"] },
    "ui": { "title": "Interpretación del clima" }
  }'
```

## Paso 2 · Crear el puente

**En la consola**: *Puentes → Nuevo puente* — slug `vigila-clima`, nombre
`Vigilante meteorológico`. **Por API**:

```bash
curl -s -X POST localhost:8080/api/v1/bridges -H "$K" -H 'content-type: application/json' \
  -d '{"slug": "vigila-clima", "name": "Vigilante meteorológico"}'
```

## Paso 3 · El trigger cron y la cadena

El trigger **cron** no tiene body ni ruta: es el scheduler interno quien
dispara. Su `schedule` es cron estándar de **5 campos** (minuto, hora, día
del mes, mes, día de la semana), con timezone IANA; admite `*`, listas
(`1,15`), rangos (`1-5`) y pasos (`*/15`); el mínimo es **1 minuto** y no se
aceptan nombres de días o meses.

**En la consola** — *Puentes → vigila-clima (editor)*:

1. **Trigger** (campo «Config (JSON)»): pega
   `{ "type": "cron", "schedule": "*/15 * * * *", "timezone": "Europe/Madrid" }`.
2. **Input schema**: déjalo vacío (`{}`) — no hay body que validar.
3. **Cadena → Añadir instancia**: `+ http-get` y `+ interpreta-clima`.
   **Renómbralas**: `traeClima` y `interpretaClima` (las refs `$.` usan esos
   name — sin renombrar, la validación falla igual que en el taller 1).
4. **b1** («Config (JSON)»): la URL fija de Open-Meteo — sin interpolación,
   porque el horario no aporta datos:
   `{ "url": "https://api.open-meteo.com/v1/forecast?latitude=40.4168&longitude=-3.7038&current=temperature_2m,wind_speed_10m", "method": "GET" }`
5. **b2** (textarea «input (refs $. hacia bloques anteriores, JSON)»):
   ```json
   {
     "temperatura": "$.traeClima.current.temperature_2m",
     "viento": "$.traeClima.current.wind_speed_10m",
     "hora": "$.traeClima.current.time"
   }
   ```
   Fíjate: en un trigger cron no existe `$.trigger.body` — el disparo no
   trae body; si lo necesitas, `$.trigger` lleva los metadatos del disparo
   (incluido `scheduled_at`).
6. **Guardar borrador** → **Validar**.

**Por API** — el mismo borrador en un `PATCH`:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE" -H "$K" \
  -H 'content-type: application/json' -d '{
  "trigger": { "type": "cron", "schedule": "*/15 * * * *", "timezone": "Europe/Madrid" },
  "blocks": [
    {
      "id": "b1", "name": "traeClima",
      "blockId": "<id de http-get>", "blockVersionId": "<versión publicada de http-get>",
      "input": {},
      "config": {
        "url": "https://api.open-meteo.com/v1/forecast?latitude=40.4168&longitude=-3.7038&current=temperature_2m,wind_speed_10m",
        "method": "GET"
      },
      "timeoutMs": 8000
    },
    {
      "id": "b2", "name": "interpretaClima",
      "blockId": "<id de interpreta-clima>", "blockVersionId": "<v1 publicada>",
      "input": {
        "temperatura": "$.traeClima.current.temperature_2m",
        "viento": "$.traeClima.current.wind_speed_10m",
        "hora": "$.traeClima.current.time"
      },
      "timeoutMs": 3000
    }
  ]
}'
```

## Paso 4 · La entrega: webhook de resultado

Para llevar el informe a otro sistema sin que nadie lo consulte, el puente
puede **entregar el resultado por webhook**: al terminar cada ejecución, la
plataforma hace POST a tu URL con el sobre del resultado, firmado y con
reintentos.

> El editor de la consola no expone hoy el webhook: se configura en el
> borrador por API, en el mismo `PATCH`.

Para probar sin montar un servidor, crea una URL desechable en
[webhook.site](https://webhook.site), cópiala y añádela al borrador:

```bash
curl -s -X PATCH "localhost:8080/api/v1/bridges/$PUENTE" -H "$K" \
  -H 'content-type: application/json' \
  -d '{"webhook": {"url": "https://webhook.site/<tu-uuid>"}}'
```

Detalles que verás en el taller de eventos a fondo: la entrega lleva las
cabeceras `X-Integraciones-Signature` (HMAC-SHA256 del body), `…-Timestamp`
y `…-Delivery` (id estable, deduplicable); los reintentos son 5, con backoff
fijo (30 s → 2 m → 10 m → 1 h → 6 h), y solo cuenta como éxito un 2xx.

## Paso 5 · Publicar y observar el disparo automático

1. **Dry-run** («Ensayo sin efectos»): deja los datos de prueba como `{}` —
   no hay `$.trigger.body` — y pulsa **Ensayar**: verás el informe en el
   output de `interpretaClima` sin ejecutar nada real.
2. **Validar** → **Publicar versión**. Desde el siguiente minuto que
   encaje en `*/15`, el scheduler encolará la ejecución.
3. **Ejecuciones**: la fila aparece sola, y en el trace el disparador es
   **`system`** — nadie la llamó. En webhook.site verás llegar el POST con
   el informe.

> **Un puente cron no se invoca por `/x`**: `POST /x/vigila-clima` responde
> 404 a propósito — su única puerta de entrada es el scheduler. Y si una
> ejecución sigue viva cuando toca la siguiente ocurrencia, esa ocurrencia
> se pierde (política anti-solape): los puentes cron no se pisan a sí mismos.

## Paso 6 · El horario es parte de la versión congelada

Cambia el horario a «laborables a las 9:00» en el borrador
(`"schedule": "0 9 * * 1-5"`) y **publica la v2**: las ejecuciones en curso
terminan con el horario con el que empezaron y el diff del panel de
versiones muestra exactamente qué cambió.

## Cuando el servidor duerme

Si el proceso cae y vuelve a arrancar, el scheduler detecta las ocurrencias
vencidas durante el apagón y **encola solo la más reciente** (para no
acumular retraso); las demás quedan registradas como
`missed_triggers` con motivo `downtime`. Tu vigilante no encadena 40
ejecuciones atrasadas tras un fin de semana apagado: se pone al día con la
última y sigue.

## Ejercicios propuestos

1. **Solo en horario laboral**: `"0 9-18 * * 1-5"` con timezone
   `Atlantic/Canary` — y comprueba en el trace que el disparador sigue
   siendo `system`.
2. **Pasos `skipped`**: añade a `b2` un
   `when: { "lt": ["$.traeClima.current.temperature_2m", 30] }` y observa en
   el trace cómo el paso queda `skipped` (no ejecutado, no error) los días
   calurosos.
3. **Duplicar el vigilante**: *Puentes → duplicar* para crear una versión
   para otra ciudad (cambia la `latitude`/`longitude` de la URL) con otro
   horario.
4. **Enfriamiento por eventos**: añade
   `emit: {"onSuccess": ["clima.lectura"]}` — el taller 4 recoge este hilo.

## Si algo falla

| Síntoma | Causa probable | Arreglo |
|---|---|---|
| No aparece ninguna ejecución | el puente no tiene versión publicada | solo corren puentes publicados: **Publicar versión** |
| | el horario aún no toca | recuerda que `*/15` dispara en los minutos 0, 15, 30, 45; para probar usa `*/1` |
| `POST /x/vigila-clima` → 404 | los puentes cron no se exponen en `/x` | correcto: el scheduler es la única puerta |
| El webhook no llega | la URL no responde 2xx o no es https | mira los reintentos (30 s → 2 m → 10 m → 1 h → 6 h) y corrige el receptor |
| `b2` falla con `REF_UNRESOLVED` | refs apuntando a nombres no renombrados | renombra las instancias (paso 3.3) |

## Checklist del taller

- [ ] `interpreta-clima` v1 publicada (código + schemas + `ui`)
- [ ] Puente `vigila-clima` con trigger cron `*/15 * * * *` en `Europe/Madrid`
- [ ] Instancias renombradas (`traeClima`, `interpretaClima`) y refs correctas
- [ ] Webhook configurado y el informe llegando a webhook.site
- [ ] Dry-run con `{}` verificado; **v1 publicada**
- [ ] Ejecución vista en *Ejecuciones* disparada por `system`
- [ ] Horario cambiado a `0 9 * * 1-5` y **v2 publicada** (diff verificado)

## Ver también

- [Taller 1: Rick and Morty](taller-rick-and-morty.md) — la base de todo
- [Taller 4: Eventos y webhooks](taller-eventos-webhooks.md) — firma HMAC y fan-out
- [Guía: crear y publicar puentes](guia-puentes.md) — trigger, `when`, webhook y mask en detalle
- [Errores comunes](errores-comunes.md) — diagnóstico rápido
