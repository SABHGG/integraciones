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

### Requirement: Disponibilidad mínima en MVP1
MVP1 SHALL incluir solo: tabla `credentials` (secreto cifrado), endpoints CRUD con write-only y la host function `ctx.credential(name)`. La rotación con historial, expiración y UI quedan para MVP3.

#### Scenario: Rotación básica
- **WHEN** se rota el valor de una credencial con `PATCH`
- **THEN** las siguientes ejecuciones de todos los puentes que la referencian usan el nuevo valor sin cambios en los puentes
