# Spec Delta

## MODIFIED Requirements

### Requirement: Formularios generados por metadatos ui
Los formularios de configuración de instancias SHALL generarse automáticamente desde el campo `ui` de la versión del bloque (etiquetas, orden, ayuda y widget por campo); un bloque sin metadatos `ui` SHALL recibir un formulario JSON genérico. Los widgets soportados SHALL cubrir el contrato completo de `ui`: `text`, `url`, `number`, `textarea`, `password`, `json` (valor estructurado editado como JSON con validación en cliente), `select` (opciones tomadas del `enum` declarado para ese campo en el `configSchema` de la versión) y `ref-template` (campo de texto con autocompletado de refs `$.` que inserta interpolación `{$.ref}`). Un id de widget desconocido SHALL caer a `text`.

#### Scenario: Formulario derivado de los metadatos
- **WHEN** el usuario añade una instancia del bloque `http-get` a un puente
- **THEN** el formulario muestra sus campos declarados en `ui` (url, headers, timeout…) con su validación

#### Scenario: Bloque sin ui metadatos
- **WHEN** un bloque `custom_logic` se publica sin campo `ui`
- **THEN** su configuración se edita como JSON libre con validación contra inputSchema al guardar

#### Scenario: Campo select con opciones del schema
- **WHEN** la versión declara `widget.method = "select"` y su `configSchema` define `enum: ["GET", "POST"]` para `method`
- **THEN** el formulario muestra un desplegable con esas opciones y guarda el valor elegido tal cual

#### Scenario: Campo json estructurado
- **WHEN** un campo declarado con widget `json` recibe su valor
- **THEN** se edita como JSON en un editor dedicado que valida la sintaxis en cliente y conserva el valor estructurado (no un string)

#### Scenario: Campo ref-template con interpolación
- **WHEN** un campo declarado con widget `ref-template` recibe una inserción de ref desde el autocompletado
- **THEN** la ref se inserta como `{$.ruta}` dentro del string y los fragmentos previos del texto se conservan

## ADDED Requirements

### Requirement: Trigger con formulario guiado por tipo
El editor del puente SHALL editar el disparador con un formulario guiado: un selector de tipo (`http`, `cron`, `event`, `manual`) y, según el tipo, solo los campos que el servidor exige. La validación en cliente SHALL marcar errores en línea (path, cron de 5 campos, zona IANA, regla de topic) sin bloquear lo que el servidor aceptaría. La edición SHALL estar disponible también como JSON avanzado, y los valores editados por una vía SHALL mantenerse al cambiar a la otra.

#### Scenario: Trigger http sin escribir JSON
- **WHEN** el usuario elige el tipo http, el verbo POST y escribe el path `/x/demo`
- **THEN** el borrador contiene `{type: "http", method: "POST", path: "/x/demo"}` y no se muestra ninguna textarea obligatoria

#### Scenario: Trigger cron con validación en línea
- **WHEN** el usuario elige cron y escribe un schedule que no es un cron de 5 campos, o una zona horaria no IANA
- **THEN** el formulario marca el error en línea antes de llegar al servidor

#### Scenario: Trigger event y manual
- **WHEN** el usuario elige event escribe un topic, o elige manual
- **THEN** event muestra solo el campo topic con su regla del servidor, y manual no muestra ningún campo adicional

#### Scenario: Ida y vuelta con el JSON avanzado
- **WHEN** el usuario pasa del formulario guiado al JSON avanzado, edita el documento y vuelve al formulario
- **THEN** los campos guiados se rellenan con lo editado y ningún valor se pierde

### Requirement: Constructor visual del inputSchema
El editor del puente SHALL editar el `inputSchema` del disparo con un constructor visual de campos (nombre, tipo `string|number|boolean|object|array` y requerido) que genera el JSON Schema equivalente, junto a una pestaña JSON avanzada. Los cambios por una vía SHALL reflejarse en la otra, y el constructor SHALL NO descartar restricciones JSON Schema que no sabe editar: lo que no sea campo simple (nombre/tipo/required) SHALL conservarse tal cual en el documento resultante.

#### Scenario: Campos declarados en el constructor
- **WHEN** el usuario añade los campos `userId` (string, requerido) y `limite` (number)
- **THEN** el borrador contiene un inputSchema con `properties` de ambos y `required: ["userId"]`

#### Scenario: Ida y vuelta con el JSON avanzado
- **WHEN** el usuario pega un schema con restricciones finas (p. ej. `pattern`) en la pestaña JSON y vuelve al constructor
- **THEN** los campos simples se muestran editables y las restricciones finas se conservan intactas al guardar

### Requirement: Autocompletado de refs $
Al editar config e input de una instancia, la consola SHALL ofrecer autocompletado de refs `$.` con las rutas disponibles en ese punto de la cadena: `$.now`, `$.trigger…` según el tipo de disparador y las salidas de los bloques previos desde su `output_schema`. En config SHALL insertarse como interpolación `{$.ruta}`; en input, como mapeo directo `$.ruta`. Sin bloques previos o sin rutas SHALL ofrecerse al menos las raíces base.

#### Scenario: Sugerencias desde bloques previos
- **WHEN** el usuario edita el config de la segunda instancia y abre el autocompletado
- **THEN** se sugieren rutas de `$.trigger…` (incluidas las propiedades que declare el `inputSchema`), `$.now` y de la salida del primer bloque según su `output_schema`

#### Scenario: Inserción en config e input
- **WHEN** el usuario acepta una sugerencia en un campo de config y otra en el input de la instancia
- **THEN** el config queda con `{$.ruta}` interpolada en el string y el input con el mapeo directo `$.ruta`

### Requirement: Autoría de metadatos ui al publicar versión de bloque
La consola SHALL permitir declarar los metadatos `ui` (title, order, widget y help del contrato vigente) al publicar una versión de bloque, y los valores declarados SHALL enviarse en la publicación y quedar visibles en la versión publicada. Sin metadatos, la publicación SHALL enviarse igual que hoy (el servidor guarda `ui = {}`).

#### Scenario: Publicar versión con ui
- **WHEN** el autor declara order y widgets de ayuda para los campos de un bloque y publica la versión
- **THEN** la petición de publicación incluye `ui` y las instancias futuras de ese bloque muestran el formulario guiado con esos campos

#### Scenario: Publicar sin ui
- **WHEN** el autor publica una versión sin tocar los metadatos ui
- **THEN** la petición es idéntica a la actual y el bloque se edita como JSON genérico
