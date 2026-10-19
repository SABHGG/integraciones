# workspace-roles Specification

## Purpose

Permisos por rol sobre el plano admin: cada miembro de un workspace (`owner`/`editor`/`runner`/`viewer`) puede exactamente lo que su rol permite, y el `owner` administra la membresía — sin que las API keys dejen de funcionar para automatización.

## Requirements

### Requirement: Matriz de permisos por rol
El plano admin SHALL aplicar la matriz: `viewer` solo lectura (`GET`); `runner` además ejecuta (disparo manual, replay, dry-run y publicación de eventos); `editor` además crea, edita y elimina recursos (bloques, puentes, credenciales, borradores) y publica versiones; `owner` además administra la membresía y las API keys. La denegación SHALL ser `403` Problem Details con el rol requerido, y la ausencia de identidad `401`.

#### Scenario: Viewer no edita
- **WHEN** un `viewer` intenta `PATCH` sobre un puente o `DELETE` sobre una credencial
- **THEN** la respuesta es `403` y el recurso permanece intacto

#### Scenario: Runner ejecuta pero no administra
- **WHEN** un `runner` dispara un puente por `POST /bridges/{id}/run` y luego intenta crear un bloque
- **THEN** el disparo se encola y la creación del bloque responde `403`

#### Scenario: Editor administra recursos sin tocar membresía
- **WHEN** un `editor` publica una versión y luego intenta cambiar el rol de un miembro
- **THEN** la publicación se crea y el cambio de rol responde `403`

### Requirement: Gestión de miembros por el owner
El sistema SHALL exponer la gestión de miembros del workspace: el listado (email, rol y fecha de alta) SHALL estar disponible para cualquier identidad autenticada del workspace; añadir por email de usuario existente, cambiar rol y expulsar SHALL estar restringidos a `owner`. Un workspace SHALL conservar siempre al menos un `owner`; el último `owner` no SHALL poder degradarse ni ser expulsado.

#### Scenario: Añadir un miembro existente
- **WHEN** el owner añade por email un usuario registrado con rol `editor`
- **THEN** la membresía queda creada y el usuario accede al workspace con ese rol

#### Scenario: Añadir un email sin cuenta
- **WHEN** el owner intenta añadir un email sin usuario registrado
- **THEN** la respuesta es `404` Problem Details invitando a que la persona se registre primero

#### Scenario: El último owner está protegido
- **WHEN** el único `owner` intenta degradarse a otro rol o expulsarse
- **THEN** la respuesta es `409` y la membresía permanece como `owner`

### Requirement: Auditoría del disparador en las ejecuciones
Cada ejecución SHALL registrar el principal que la disparó (`user` con email o `api_key` con nombre), expuesto en el detalle de la ejecución, de modo que se pueda responder quién lanzó qué.

#### Scenario: Ejecución por usuario identificada
- **WHEN** un miembro dispara un puente y se consulta el detalle de la ejecución
- **THEN** el detalle incluye el disparador con su email y tipo `user`
