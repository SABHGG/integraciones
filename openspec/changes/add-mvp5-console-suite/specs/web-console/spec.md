# Spec Delta: web-console

## Purpose

Cerrar la cobertura de la consola sobre el producto: credenciales, biblioteca de bloques, versiones con diff/rollback, métricas en vivo y utilidades de export/import — consumiendo el plano admin (más los dos endpoints de lectura nuevos) sin inventar validación propia.

## ADDED Requirements

### Requirement: Página de credenciales
La consola SHALL incluir una vista de credenciales del workspace: listado con nombre, descripción y versión vigente; creación; rotación (que nunca muestre el valor anterior); eliminación con confirmación; e historial de versiones por credencial (ids y fechas, jamás valores). La vista SHALL respetar el write-only de la bóveda en todas sus superficies. La edición SHALL quedar reservada a `editor`; `viewer` la ve en lectura.

#### Scenario: Rotar desde la consola
- **WHEN** el usuario rota una credencial con un valor nuevo
- **THEN** la consola muestra la nueva versión vigente y el historial lista ambas versiones con sus fechas, sin ningún valor

#### Scenario: Viewer en lectura
- **WHEN** entra un `viewer` a la página de credenciales
- **THEN** ve el listado y el historial (ids y fechas) sin botones de crear, rotar ni eliminar

#### Scenario: Historial sin valores
- **WHEN** se abre el historial de una credencial
- **THEN** la vista muestra ids y fechas; ningún endpoint de la bóveda es invocado de forma que pueda revelar un valor

### Requirement: Biblioteca de bloques
La consola SHALL incluir una vista de la biblioteca de bloques: listado (slug, tipo, categoría, versión vigente), detalle de versiones (schemas y metadatos `ui`), creación de bloques `custom_logic` y edición de su código con el editor con errores del validador en línea, y publicación de versiones con los errores del servidor visibles antes de publicar. Crear, editar y publicar SHALL quedar reservados a `editor`; `viewer` inspecciona la biblioteca en lectura.

#### Scenario: Crear y publicar un bloque custom_logic
- **WHEN** el usuario crea un bloque `custom_logic`, escribe su código TypeScript y publica una versión con un error de transpile
- **THEN** el editor muestra el error con su línea y no se publica; al corregirlo, la versión se publica

#### Scenario: Inspeccionar un bloque built-in
- **WHEN** el usuario abre el detalle de un bloque `http` de la biblioteca base
- **THEN** ve sus versiones con config/input/output schema y los metadatos `ui` que usan los formularios de instancias

#### Scenario: Viewer sin controles de escritura
- **WHEN** entra un `viewer` a la biblioteca de bloques
- **THEN** ve el listado y los detalles sin botones de crear bloque, editar código ni publicar versión

### Requirement: Versiones con diff y rollback
La consola SHALL listar las versiones publicadas de un puente, mostrar un diff visual lado a lado entre dos versiones o entre borrador y versión, y permitir rollback: aplicar la definición de una versión anterior al borrador y publicarla como versión nueva (marcada como tal en la UI), usando la definición congelada del plano. El diff SHALL ser accesible en lectura (`viewer`); el rollback — que publica — SHALL quedar reservado a `editor`.

#### Scenario: Comparar borrador con la versión publicada
- **WHEN** el usuario compara el borrador actual contra la última versión publicada
- **THEN** el diff muestra las diferencias (trigger, bloques, mask, emit, webhook) y permite adoptar la definición publicada al borrador

#### Scenario: Rollback a una versión anterior
- **WHEN** el usuario ejecuta el rollback a la versión 2 de un puente con 5 versiones
- **THEN** se publica una versión 6 con el contenido congelado de la 2 y la UI lo etiqueta como rollback; las versiones 1-5 y sus ejecuciones quedan intactas

#### Scenario: Viewer compara sin rollback
- **WHEN** un `viewer` abre la vista de versiones
- **THEN** ve la lista y el diff sin botón de rollback ni adopción al borrador

### Requirement: Dashboard con métricas y auto-refresh
El dashboard SHALL mostrar las métricas agregadas del workspace (`GET /stats`): total y estados, serie diaria y duración media, con filtro de rango de fechas y desglose por puente navegable al listado filtrado. El listado de ejecuciones SHALL ofrecer auto-refresh por polling configurable (apagado por defecto).

#### Scenario: Salud del workspace de un vistazo
- **WHEN** el usuario abre el dashboard
- **THEN** ve total, tasa de éxito y tendencia diaria del rango elegido, y puede saltar al listado de un puente concreto

#### Scenario: Auto-refresh en vivo
- **WHEN** el usuario activa el auto-refresh en el listado
- **THEN** el listado se refresca periódicamente por polling y el indicador muestra la última actualización; apagado, no hay polling

### Requirement: Export, import y duplicar puentes
La consola SHALL exportar un puente (borrador o una versión publicada) como JSON descargable, importarlo creando un puente nuevo (borrador válido + publicación opcional) y duplicar un puente existente en un clic. El import SHALL validar contra el plano (validación del servidor como única fuente de verdad) y mostrar los errores de validación sobre el formulario. Exportar SHALL ser lectura; importar y duplicar crean recursos, así que SHALL quedar reservados a `editor`.

#### Scenario: Exportar y compartir
- **WHEN** el usuario exporta la versión 3 de un puente
- **THEN** descarga un JSON con la definición congelada (sin ids de workspace) listo para importar en otro workspace

#### Scenario: Import con validación del servidor
- **WHEN** el usuario importa un JSON cuyo borrador tiene una ref rota
- **THEN** la consola crea el puente, ejecuta la validación del servidor y muestra los errores; no publica nada hasta que el borrador es válido

#### Scenario: Viewer exporta pero no crea
- **WHEN** un `viewer` abre la lista de puentes
- **THEN** puede exportar (lectura) pero no ve los botones de importar ni duplicar
