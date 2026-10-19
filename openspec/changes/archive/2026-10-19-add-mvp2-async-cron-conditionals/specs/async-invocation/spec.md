# Spec Delta: async-invocation

## Purpose

Modo asíncrono del plano de invocación: el llamador descarga la espera — el disparo responde `202` en cuanto la ejecución queda encolada y el resultado se consulta después por polling, sin que un puente lento bloque al cliente ni consuma el timeout del transporte.

## ADDED Requirements

### Requirement: Disparo asíncrono opt-in por petición
El sistema SHALL aceptar el modo asíncrono en `POST /x/{slug}` mediante el header `Prefer: respond-async` (RFC 7240): la petición SHALL responder `202` en cuanto la ejecución quede encolada, con el sobre `{success: true, executionId}` y un header `Location` apuntando al recurso de resultado. El modo síncrono (200 con sobre final) SHALL seguir siendo el comportamiento por defecto e inalterado.

#### Scenario: Disparo async encola y responde 202
- **WHEN** se invoca `POST /x/pedidos` con `Prefer: respond-async` y body válido
- **THEN** la respuesta es `202` con `{success: true, executionId}` y `Location: /x/pedidos/executions/{executionId}`
- **AND** la ejecución nace `pending` en la cola sin que el llamador espere su fin

#### Scenario: La idempotencia funciona igual en async
- **WHEN** se reintenta el disparo async con el mismo `Idempotency-Key` tras un timeout de red
- **THEN** se devuelve `202` con el `executionId` de la ejecución original y `Location` de la misma, sin crear una nueva ejecución

#### Scenario: Los errores de transporte no cambian
- **WHEN** el disparo async usa un slug inexistente o un method que no coincide con el trigger
- **THEN** recibe el mismo 404/405 Problem Details del plano síncrono, antes de encolar nada

### Requirement: Recurso de resultado por polling
El sistema SHALL exponer `GET /x/{slug}/executions/{executionId}` para consultar el resultado sin auth de administración: mientras la ejecución está `pending` o `running` responde `200` con `{success: true, executionId, status}`; al terminar responde con el sobre final completo `{success, data, error?, executionId}` (idéntico al que habría devuelto la llamada síncrona).

#### Scenario: Polling durante la ejecución
- **WHEN** se consulta el recurso de resultado de una ejecución aún `running`
- **THEN** la respuesta es `200` con `{success: true, executionId, status: "running"}` y sin `data`

#### Scenario: Polling del resultado final
- **WHEN** se consulta el recurso de resultado de una ejecución terminada
- **THEN** la respuesta es el sobre final (`success`, `data` o `error.code` estable, `executionId`)

#### Scenario: El recurso queda acotado al slug
- **WHEN** se consulta `/x/otro-slug/executions/{executionId}` con un executionId que pertenece a otro puente
- **THEN** la respuesta es `404` Problem Details, sin filtrar datos de otros puentes
