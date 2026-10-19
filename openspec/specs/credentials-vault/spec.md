# Credentials Vault Specification

## Purpose

Bóveda de credenciales compartida por workspace: los secretos se guardan cifrados en reposo, solo se aceptan en escritura (nunca se devuelven) y se resuelven en el host por referencia en el momento de la ejecución, de modo que jamás aparecen en el IO persistido de los puentes.

## Requirements

### Requirement: Bóveda compartida por referencia
El sistema SHALL almacenar credenciales por workspace en una bóveda (`credentials`) compartibles entre todos los miembros del workspace: los bloques las referencian por nombre/ID y el host resuelve el valor al momento de la llamada. Las credenciales NO SHALL duplicarse por usuario.

#### Scenario: Credencial compartida dentro del workspace
- **WHEN** dos puentes del mismo workspace configuran un bloque HTTP con la misma credencial referenciada
- **THEN** ambos resuelven el mismo secreto almacenado una sola vez en la bóveda

### Requirement: Secreto write-only
La API SHALL aceptar el valor del secreto solo en creación y rotación (`POST`/`PATCH /api/v1/credentials`), cifrado en reposo, y SHALL negarse a devolverlo en cualquier lectura (solo metadatos). El valor resuelto SHALL entregarse al bloque únicamente dentro del host, sin entrar al contexto, al IO entre bloques ni al store de ejecuciones.

#### Scenario: El secreto jamás sale por la API ni vive en el IO
- **WHEN** un bloque usa `ctx.credential("stripe_key")` y la ejecución se persiste
- **THEN** el trace contiene la referencia y el resultado de la llamada, pero nunca el valor del secreto
- **AND** ninguna consulta a `/api/v1/credentials` devuelve el valor

### Requirement: Versionado y rotación de secretos
El sistema SHALL versionar cada credencial: toda escritura del valor (`POST`/`PATCH`) SHALL crear una versión inmutable cifrada con su fecha de rotación, la versión más reciente SHALL ser la vigente que resuelve `ctx.credential(name)`, y las ejecuciones SHALL registrar qué versión de cada credencial usaron (identificador de versión, jamás el valor). El listado SHALL exponer el historial de versiones como metadatos, nunca valores.

#### Scenario: Rotar sin tocar los puentes
- **WHEN** se rota el valor de una credencial con `PATCH` creando la versión 2
- **THEN** las siguientes ejecuciones de todos los puentes que la referencian resuelven la versión 2 sin cambios en ninguna definición

#### Scenario: Auditoría de la versión usada
- **WHEN** se consulta el detalle de una ejecución anterior a una rotación
- **THEN** la ejecución registra el id de la versión de credencial que usó, verificable contra el historial

#### Scenario: El historial no revela valores
- **WHEN** se consulta el historial de versiones de una credencial
- **THEN** se listan metadatos (ids, fechas de rotación) y ninguna versión devuelve su valor
