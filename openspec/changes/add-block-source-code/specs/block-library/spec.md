# block-library Delta

## MODIFIED Requirements

### Requirement: Bloques como biblioteca reutilizable
El sistema SHALL mantener un registro central de bloques reutilizables (`blocks`) organizados por workspace, cada uno con metadatos (nombre, slug, tipo `http`|`custom_logic`, categoría, descripción, icono) y un conjunto de versiones inmutables. Para `custom_logic`, la versión inmutable SHALL conservar el fuente TypeScript original además del JavaScript transpilado.

#### Scenario: Crear y publicar un bloque
- **WHEN** un cliente crea un bloque CUSTOM_LOGIC con código TypeScript y publica `POST /api/v1/blocks/{id}/versions`
- **THEN** el sistema transpila el TS a JS con swc, valida sintaxis, inputSchema/outputSchema/configSchema y metadatos `ui` contra el subconjunto estricto de JSON Schema
- **AND** guarda una versión inmutable `UNIQUE(block_id, version)` con el JS transpilado y el fuente TypeScript original que se envió, y asigna `latest_version`
- **AND** los errores de compilación o de schema se devuelven como 422 Problem Details sin crear la versión

#### Scenario: Bloque referenciado no se puede eliminar
- **WHEN** se solicita `DELETE /api/v1/blocks/{id}` de un bloque referenciado por algún puente
- **THEN** el sistema responde 409 Problem Details y no elimina nada

## ADDED Requirements

### Requirement: Fuente original preservado junto al JS transpilado
La publicación de un bloque `custom_logic` SHALL conservar el fuente TypeScript original junto al JavaScript transpilado en la versión inmutable, y toda lectura de versiones SHALL exponer ambos artefactos por separado: el JS ejecutable y el fuente original. El runtime SHALL seguir ejecutando únicamente el JavaScript transpilado. La conservación SHALL ser aditiva: ninguna versión previa ni ningún bloque built-in recibe un fuente retroactivo, inferido o sintético.

#### Scenario: Publicar conserva ambos artefactos
- **WHEN** un cliente publica una versión nueva de un bloque `custom_logic` enviando su TypeScript
- **THEN** la versión guarda el fuente original tal como se envió y el JavaScript transpilado por swc
- **AND** la versión publicada en la respuesta incluye el fuente original junto al JS, y la lectura de versiones del bloque los devuelve en campos separados

#### Scenario: El runtime no ve TypeScript
- **WHEN** un puente ejecuta un paso que usa una versión de bloque `custom_logic` publicada con este change
- **THEN** el sandbox recibe y ejecuta el JavaScript transpilado, sin acceso al fuente TypeScript

#### Scenario: Built-in http sin fuente
- **WHEN** se publica una versión de un bloque built-in `http`
- **THEN** la versión queda sin código y sin fuente original (su comportamiento lo define el host)
- **AND** la lectura de versiones lo expresa como ausencia de ambos artefactos, no como un fuente vacío

#### Scenario: Versión histórica anterior al change
- **WHEN** se lee una versión de un bloque `custom_logic` publicada antes de este change
- **THEN** la versión expone el JavaScript transpilado que ya guardaba y ningún fuente original
- **AND** el sistema no inventa ni reconstruye un fuente retroactivo
