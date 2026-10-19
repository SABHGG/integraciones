# event-triggers Specification

## Purpose
Composición de puentes por eventos: un puente puede suscribirse a un topic y ejecutarse al publicarse eventos en él — por API o emitidos por otros puentes al terminar — encadenando integraciones sin acoplar puentes entre sí.

## Requirements

### Requirement: Suscripción de puentes a topics
El sistema SHALL aceptar `{type: "event", topic}` como trigger del puente, con topic validado al publicar (formato slug: minúsculas, puntos y guiones) y congelado en el snapshot. Varios puentes SHALL poder suscribirse al mismo topic.

#### Scenario: Publicar puente suscrito a un topic
- **WHEN** se publica un puente con `{type: "event", topic: "pedido.creado"}`
- **THEN** la versión congela el topic y el puente queda suscrito a eventos de `pedido.creado`

#### Scenario: Topic inválido bloquea la publicación
- **WHEN** el trigger declara `topic: "Pedido Creado!!"`
- **THEN** la publicación falla con 422 Problem Details indicando el formato de topic

### Requirement: Publicación de eventos con fan-out
El sistema SHALL exponer `POST /api/v1/events/{topic}` en el plano de administración: publica un evento con el body como payload y encola una ejecución (`trigger_type: "event"`, `trigger_data` con payload e id de evento) por cada puente suscrito activo. Sin suscriptores SHALL aceptarse la publicación (`202`, cero ejecuciones).

#### Scenario: Fan-out a varios suscriptores
- **WHEN** se publica un evento en `pedido.creado` con tres puentes suscritos y publicados
- **THEN** se encolan tres ejecuciones independientes, cada una con su puente y versión activos y el mismo payload

#### Scenario: Topic sin suscriptores
- **WHEN** se publica un evento en un topic sin puentes suscritos
- **THEN** la respuesta es `202` sin ejecuciones creadas y el evento no se retiene

### Requirement: Emisión de eventos al completar
La definición del puente SHALL admitir `emit: {onSuccess: [topics], onFailure: [topics]}` (congelada en el snapshot): al terminar una ejecución el sistema SHALL publicar los eventos configurados con el sobre final como payload, enriquecido con `bridgeSlug` y `executionId` del emisor.

#### Scenario: Dos puentes encadenados por evento
- **WHEN** el puente A declara `emit.onSuccess: ["pedido.creado"]` y el puente B está suscrito a ese topic
- **THEN** al terminar con éxito una ejecución de A se encola una ejecución de B con el resultado de A como payload

#### Scenario: Emisión configurada solo para fallos
- **WHEN** el puente declara `emit: {onFailure: ["integracion.fallo"]}` y la ejecución termina failed
- **THEN** se publica el evento de fallo con `error.code` y `executionId`, y ningún evento se emite en ejecuciones exitosas
