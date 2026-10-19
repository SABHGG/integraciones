# Spec Delta: webhook-delivery

## Purpose

Entrega push del resultado del puente: cuando la ejecución termina, el sistema envía el sobre final por HTTP a la URL configurada en el puente, firmado con HMAC-SHA256 y con reintentos persistentes — el consumidor no necesita hacer polling ni mantener la conexión abierta.

## ADDED Requirements

### Requirement: Webhook configurado en el puente
La definición del puente SHALL admitir una sección `webhook: {url}` en el borrador, con URL `https` obligatoria, validada al publicar y conservada en el snapshot de la versión. El sistema SHALL generar un secreto HMAC por puente, visible únicamente por el plano de administración, que el consumidor usa para verificar la firma.

#### Scenario: Publicar puente con webhook
- **WHEN** se publica un puente cuya definición declara `webhook.url` válida
- **THEN** la versión inmutable congela la configuración y el plano de administración expone el secreto HMAC del puente

#### Scenario: URL no https bloquea la publicación
- **WHEN** la definición declara `webhook: {url: "http://…"}` (sin TLS)
- **THEN** la publicación falla con 422 Problem Details

### Requirement: Entrega firmada del resultado
Al terminar una ejecución de un puente con webhook configurado — con éxito o con fallo — el sistema SHALL enviar `POST` a la URL registrada con el sobre final (`{success, data?, error?, executionId}`) más metadatos (`bridgeSlug`, `status`, `finishedAt`). La petición SHALL llevar los headers `X-Integraciones-Signature: sha256=<hmac>` (HMAC-SHA256 del body crudo con el secreto del puente) y `X-Integraciones-Timestamp` para verificar frescura y autenticidad.

#### Scenario: Entrega verificable por el consumidor
- **WHEN** una ejecución termina y el puente tiene webhook configurado
- **THEN** el receptor recibe un `POST` cuyo body coincide con la firma HMAC calculada con su secreto compartido

#### Scenario: El fallo de ejecución también se entrega
- **WHEN** una ejecución termina en `failed` con error `BLOCK_FAILED`
- **THEN** el webhook entrega el sobre con `success: false`, `error.code` y `executionId`

### Requirement: Reintentos persistentes con backoff
Cada entrega SHALL registrarse en `webhook_deliveries` con estado `pending → delivered|failed`, contador de intentos, código de respuesta y último error. Un intento SHALL considerarse exitoso solo con respuesta `2xx`; ante fallo o timeout SHALL reintentar con backoff exponencial (30 s, 2 min, 10 min, 1 h, 6 h) hasta agotar 5 intentos, quedando `failed` con el último `response_status` registrado.

#### Scenario: Un 500 programa el reintento
- **WHEN** el receptor responde `500` a la primera entrega
- **THEN** la entrega queda `pending` con `next_retry_at` a ~30 s y `attempts: 1`

#### Scenario: Intentos agotados marcan la entrega fallida
- **WHEN** el receptor sigue fallando tras el quinto intento
- **THEN** la entrega queda `failed` con el último código de respuesta y no se reintenta de nuevo

### Requirement: Entregas deduplicables
Cada intento de entrega SHALL llevar el header `X-Integraciones-Delivery` con el identificador estable de la entrega, de modo que un receptor pueda deduplicar los reintentos de un mismo resultado.

#### Scenario: El reintento es reconocible como el mismo envío
- **WHEN** el receptor compara los headers de la entrega original y de su reintento
- **THEN** ambos comparten el mismo `X-Integraciones-Delivery` y `executionId`
