# Block Library

## ADDED Requirements

### Requirement: Bloques como biblioteca reutilizable
El sistema SHALL mantener un registro central de bloques reutilizables (`blocks`) organizados por workspace, cada uno con metadatos (nombre, slug, tipo `http`|`custom_logic`, categoría, descripción, icono) y un conjunto de versiones inmutables.

#### Scenario: Crear y publicar un bloque
- **WHEN** un cliente crea un bloque CUSTOM_LOGIC con código TypeScript y publica `POST /api/v1/blocks/{id}/versions`
- **THEN** el sistema transpila el TS a JS con swc, valida sintaxis, inputSchema/outputSchema/configSchema y metadatos `ui` contra el subconjunto estricto de JSON Schema
- **AND** guarda una versión inmutable `UNIQUE(block_id, version)` con el JS resultante y asigna `latest_version`
- **AND** los errores de compilación o de schema se devuelven como 422 Problem Details sin crear la versión

#### Scenario: Bloque referenciado no se puede eliminar
- **WHEN** se solicita `DELETE /api/v1/blocks/{id}` de un bloque referenciado por algún puente
- **THEN** el sistema responde 409 Problem Details y no elimina nada

### Requirement: Convención de autoría del bloque
El sistema SHALL definir el contrato de un bloque CUSTOM_LOGIC como una función única con export por defecto que recibe `(input, ctx)` y devuelve la salida o una Promise. Los metadatos y la configuración de instancia SHALL vivir fuera del código (en `BlockDefinition` y en la instancia del puente respectivamente).

#### Scenario: El código del bloque solo transforma
- **WHEN** el ejecutor invoca un bloque
- **THEN** el código recibe únicamente `input` (resuelto y validado) y `ctx` (capacidades del host)
- **AND** no existe acceso a sistema de archivos, procesos, red directa ni módulos externos

### Requirement: Capacidades del sandbox expuestas por ctx
El sistema SHALL exponer a los bloques únicamente las capacidades provistas por host functions: `ctx.http.request` (con timeout obligatorio y allowlist opcional de dominios), `ctx.log` estructurado, `ctx.crypto` y helpers JSON. No SHALL haber soporte npm en MVP1.

#### Scenario: Llamada HTTP saliente
- **WHEN** un bloque invoca `ctx.http.request` con un dominio dentro de la allowlist del workspace
- **THEN** el host realiza la petición (reqwest) con deadline propio y devuelve el cuerpo como texto o JSON parseado
- **AND** si el dominio está fuera de la allowlist, la llamada falla con error explícito
