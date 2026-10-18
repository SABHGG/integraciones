# Bridge Invocation Specification

## Purpose

Plano público de invocación (`/x/{slug}`) que ejecuta puentes publicados de forma síncrona, separado del plano de administración. Define el contrato de respuesta (sobre único con `success/data/error/executionId`), los errores de transporte RFC 9457 y la idempotencia opcional por `Idempotency-Key`.

## Requirements

### Requirement: Plano de invocación separado
El sistema SHALL exponer los puentes publicados bajo un plano de invocación propio (`/x/{slug}`) separado del plano de administración (`/api/v1`), con middlewares y rate-limits independientes. El plano de invocación SHALL ignorar la auth de administración; la auth por puente queda para una versión futura.

#### Scenario: Superficies aisladas
- **WHEN** un llamador del plano `/x/{slug}` intenta alcanzar cualquier endpoint de `/api/v1`
- **THEN** la petición no resuelve: son planos distintos sin recursos compartidos de superficie

### Requirement: Contrato de respuesta síncrona
El sistema SHALL responder a una invocación síncrona con 200 y el sobre único `{success, data, error?, executionId}` en todos los casos, éxito o fallo de ejecución. Los códigos de error SHALL ser estables (enum: `BLOCK_FAILED`, `VALIDATION_FAILED`, `TIMEOUT`, `NOT_FOUND`, …) y forman contrato para los consumidores.

#### Scenario: Ejecución exitosa
- **WHEN** un puente publica `POST /x/{slug}` y su cadena termina correctamente
- **THEN** la respuesta es 200 con `success: true`, `data` = salida del último bloque y `executionId`

#### Scenario: Ejecución fallida con sobre 200
- **WHEN** un bloque falla durante la ejecución
- **THEN** la respuesta es 200 con `success: false`, `error.code: "BLOCK_FAILED"`, el bloque que falló, mensaje y `executionId`

### Requirement: Errores de transporte con semántica HTTP
Los fallos de transporte SHALL usar HTTP semántico con Problem Details RFC 9457: 404 (slug inexistente), 405 (método no coincide con el trigger), 413 (body excede el límite), 415 (Content-Type no soportado, solo `application/json` en v1).

#### Scenario: Slug inexistente
- **WHEN** se invoca `/x/no-existe`
- **THEN** la respuesta es 404 con Problem Details, sin el sobre de ejecución

### Requirement: Idempotencia opcional
El sistema SHALL aceptar un header `Idempotency-Key` opcional en el disparo HTTP; si la key se repite, SHALL devolver el resultado de la ejecución previa en lugar de ejecutar de nuevo. SHALL existir unique parcial sobre `idempotency_key`.

#### Scenario: Reintento del cliente no duplica efectos
- **WHEN** un cliente reintenta la invocación con el mismo `Idempotency-Key` tras un timeout
- **THEN** recibe el resultado de la ejecución original y no se crea una nueva ejecución
