# Errores comunes

Diagnóstico rápido: el síntoma que ves, su causa y qué hacer. Los errores
del plano son Problem Details (`title`, `detail`, `status`); los de las
ejecuciones llevan un `code` estable.

## Acceso y sesión

| Síntoma | Causa | Qué hacer |
|---|---|---|
| Login responde `401` «credenciales inválidas» | Email o contraseña incorrectos. El error es **uniforme**: no distingue si el email existe. | Reintenta; si no tienes cuenta, pide a un `owner` que te añada **después** de registrarte (`POST /api/v1/auth/register`). |
| Tu token deja de funcionar con `401` en todo | Sesión revocada (logout), expirada (30 días) o **te expulsaron del workspace**. | Vuelve a entrar; si no puedes, te quitaron la membresía: habla con un `owner`. |
| Entraste pero no ves ningún dato | La sesión quedó sin workspace fijado (perteneces a varios) o fijada a otro. | Cierra sesión y entra de nuevo eligiendo el workspace. |
| `403` «esta acción requiere rol X y tu identidad tiene Y» | La matriz de roles denegó la acción. El mensaje nombra el rol que falta. | Pide a un `owner` el cambio de rol en *Miembros* (efectivo al instante). |

## Crear recursos

| Síntoma | Causa | Qué hacer |
|---|---|---|
| `409` «el slug … ya existe en este workspace» | Slug repetido (puente, bloque). | Elige otro slug o usa **duplicar**, que genera `slug-copia`. |
| `404` al añadir un miembro por email | La persona **no tiene cuenta todavía**. | Que se registre primero; después el `owner` la añade. |
| `409` al degradar o expulsar | Es el **último owner** del workspace. | Añade otro owner antes; el workspace siempre conserva uno. |
| `422` con `errors[]` por campo | El validador rechazó el cuerpo (campos obligatorios, formatos). | Corrige los campos señalados — en la consola aparecen en línea bajo cada campo. |
| `409` al hacer login con varias membresías | Falta elegir workspace. | Repite el login con `"workspace": "{slug}"` (la consola te lo pregunta). |

## Invocar puentes

| Síntoma | Causa | Qué hacer |
|---|---|---|
| `404` en `POST /x/{ruta}` (y el puente existe) | Dos causas **indistinguibles a propósito**: el despacho es por **slug del puente** (no por `trigger.path`) o el puente **no tiene versión publicada**. | Revisa que invocas `POST /x/{slug}` y que el puente tiene `latestVersion`. Los puentes `cron` **nunca** responden en `/x` (404 también). |
| `422` al invocar: el body no pasa `inputSchema` | El disparo no cumple el schema del puente. | Corrige el body; el `detail` señala el campo. |
| La ejecución queda `pending` eternamente | En local, el worker no corre o la cola tiene filas viejas de otro test/BD. | En producción es la cola normal; en local revisa que el servidor está arriba (el worker vive en el mismo binario). |

## Ejecuciones `failed` (códigos estables)

| `code` | Significado | Qué hacer |
|---|---|---|
| `REF_UNRESOLVED` | Una referencia `$.` no resolvió: mal escrita, apunta a un bloque **posterior** o al campo equivocado de la salida. | Abre el trace: el paso fallido nombra la referencia. Compárala con la salida real del bloque anterior (visible en el trace). |
| `TIMEOUT` | El bloque superó su plazo (por defecto 5 s). | Sube `timeoutMs` de esa instancia o revisa la llamada externa. |
| `BLOCK_FAILED` | El código del bloque lanzó un error (o el built-in `http` falló: DNS, status, dominio no permitido). | El `detail` del paso trae el mensaje; en `custom_logic` aparece tu `throw` o el error de `ctx.http`. |
| `VALIDATION_FAILED` | La salida del bloque no cumplió su `outputSchema`, o un paso siguiente esperaba otra forma. | Ajusta el código o el schema; el trace muestra la salida obtenida. |

Regla general: el trace **nunca miente** — el paso marcado `failed` es el
primero que falló, con su input tal como llegó (ya enmascarado).

## Entregas y eventos

| Síntoma | Causa | Qué hacer |
|---|---|---|
| El webhook no llega | URL no-https (se valida al publicar), destino caído (hay **reintentos** con backoff) o firma rechazada. | Verifica `https`, y valida la firma `X-Integraciones-Signature` (HMAC-SHA256 con el secreto del puente; `X-Integraciones-Timestamp` y `X-Integraciones-Delivery` acompañan). |
| El puente por evento no se dispara | Nadie publica su topic, o el puente no está publicado. | `POST /api/v1/events/{topic}` responde `202` con el número de suscriptores: `0` significa que ningún puente publicado escucha ese topic. |
| `emit` no encadena | Los topics solo se publican al **completar** (éxito → `onSuccess`, fallo → `onFailure`). | Revisa el detalle de la ejecución: el evento emitido queda registrado en el resultado. |

## Métricas y datos

| Síntoma | Causa | Qué hacer |
|---|---|---|
| Días con cero en la tendencia diaria | Son huecos reales: la API solo devuelve días con ejecuciones y la consola los rellena a cero. | Normal. |
| `400` «el rango no puede superar los 90 días» | Límite de `GET /stats` para acotar coste. | Consulta en rangos menores o pagina por tramos. |
| Métricas lentas con muchísimas ejecuciones | Agregación directa sin cache (decisión de diseño v1). | Reduce el rango; el límite de 90 días protege el peor caso. |

## Ver también

- [`guia-consola.md`](guia-consola.md) — páginas, roles y flujos de la consola
- [`guia-bloques.md`](guia-bloques.md) — crear bloques · [`guia-puentes.md`](guia-puentes.md) — crear puentes
- Referencia completa del API: `/docs` (Scalar) en tu instancia
