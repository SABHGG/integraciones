# web-console Delta

## ADDED Requirements

### Requirement: Código de cada versión en el detalle del bloque
El detalle de un bloque SHALL mostrar el código de cada versión listada, en lectura, con un visor por versión. Cuando la versión conserve fuente original, el visor SHALL mostrar ese fuente; cuando solo exista el JavaScript transpilado, SHALL mostrarlo identificándolo como tal; cuando la versión no tenga código, SHALL indicarlo explícitamente. El visor SHALL estar disponible para cualquier rol que lea el bloque.

#### Scenario: Versión con fuente original
- **WHEN** el usuario abre el detalle de un bloque `custom_logic` con una versión publicada después de este change
- **THEN** el visor de esa versión muestra el TypeScript original, y no el JavaScript emitido

#### Scenario: Versión anterior al change
- **WHEN** el usuario abre una versión `custom_logic` publicada antes de este change, que solo guarda JavaScript
- **THEN** el visor muestra ese JavaScript con una etiqueta que advierte que es el JS transpilado y que la versión es anterior al change

#### Scenario: Bloque built-in sin código
- **WHEN** el usuario abre el detalle de un bloque built-in `http`
- **THEN** el visor de sus versiones indica que el bloque no tiene código, sin mostrar un visor vacío

#### Scenario: Viewer lee el código
- **WHEN** un `viewer` abre el detalle de un bloque
- **THEN** ve el código de las versiones en lectura, sin editor ni botón de publicar

### Requirement: Editor de nueva versión precargado con el fuente vigente
El editor de "publicar nueva versión" de un bloque `custom_logic` SHALL precargarse con el fuente original de la última versión del bloque, de modo que editar y publicar parta del código real y no de una plantilla. Solo cuando la última versión no conserve fuente original — versión anterior al change o bloque sin versiones — SHALL usarse la plantilla por defecto.

#### Scenario: Continuar desde la última versión
- **WHEN** un `editor` abre el detalle de un bloque cuya última versión guarda fuente original
- **THEN** el editor aparece con ese fuente ya cargado y publicar crea la siguiente versión a partir de él

#### Scenario: Sin fuente disponible
- **WHEN** un `editor` abre el detalle de un bloque sin versiones, o cuya última versión solo guarda JavaScript
- **THEN** el editor aparece con la plantilla por defecto, sin presentar el JavaScript como si fuera el fuente
