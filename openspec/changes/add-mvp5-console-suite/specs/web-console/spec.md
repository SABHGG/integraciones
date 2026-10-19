# Spec Delta: web-console

## Purpose

Cerrar la cobertura de la consola sobre el producto: credenciales, biblioteca de bloques, versiones con diff/rollback, métricas en vivo y utilidades de export/import — consumiendo el plano admin (más los dos endpoints de lectura nuevos) sin inventar validación propia.

## ADDED Requirements

### Requirement: Página de credenciales
La consola SHALL incluir una vista de credenciales del workspace: listado con nombre, descripción y versión vigente; creación; rotación (que nunca muestre el valor anterior); eliminación con confirmación; e historial de versiones por credencial (ids y fechas, jamás valores). La vista SHALL respetar el write-only de la bóveda en todas sus superficies.

#### Scenario: Rotar desde la consola
- **WHEN** el usuario rota una credencial con un valor nuevo
- **THEN** la consola muestra la nueva versión vigente y el historial lista ambas versiones con sus fechas, sin ningún valor

#### Scenario: Historial sin valores
- **WHEN** se abre el historial de una credencial
- **THEN** la vista muestra ids y fechas; ningún endpoint de la bóveda es invocado de forma que pueda revelar un valor

### Requirement: Biblioteca de bloques
La consola SHALL incluir una vista de la biblioteca de bloques: listado (slug, tipo, categoría, versión vigente), detalle de versiones (schemas y metadatos `ui`), creación de bloques `custom_logic` y edición de su código con el editor con errores del validador en línea, y publicación de versiones con los errores del servidor visibles antes de publicar.

#### Scenario: Crear y publicar un bloque custom_logic
- **WHEN** el usuario crea un bloque `custom_logic`, escribe su código TypeScript y publica una versión con un error de transpile
- **THEN** el editor muestra el error con su línea y no se publica; al corregirlo, la versión se publica

#### Scenario: Inspeccionar un bloque built-in
- **WHEN** el usuario abre el detalle de un bloque `http` de la biblioteca base
- **THEN** ve sus versiones con config/input/output schema y los metadatos `ui` que usan los formularios de instancias

### Requirement: Versiones con diff y rollback
La consola SHALL listar las versiones publicadas de un puente, mostrar un diff visual lado a lado entre dos versiones o entre borrador y versión, y permitir rollback: aplicar la definición de una versión anterior al borrador y publicarla como versión nueva (marcada como tal en la UI), usando la definición congelada del plano.

#### Scenario: Comparar borrador con la versión publicada
- **WHEN** el usuario compara el borrador actual contra la última versión publicada
- **THEN** el diff muestra las diferencias (trigger, bloques, mask, emit, webhook) y permite adoptar la definición publicada al borrador

#### Scenario: Rollback a una versión anterior
- **WHEN** el usuario ejecuta el rollback a la versión 2 de un puente con 5 versiones
- **THEN** se publica una versión 6 con el contenido congelado de la 2 y la UI lo etiqueta como rollback; las versiones 1-5 y sus ejecuciones quedan intactas

### Requirement: Dashboard con métricas y auto-refresh
El dashboard SHALL mostrar las métricas agregadas del workspace (`GET /stats`): total y estados, serie diaria y duración media, con filtro de rango de fechas y desglose por puente navegable al listado filtrado. El listado de ejecuciones SHALL ofrecer auto-refresh por polling configurable (apagado por defecto).

#### Scenario: Salud del workspace de un vistazo
- **WHEN** el usuario abre el dashboard
- **THEN** ve total, tasa de éxito y tendencia diaria del rango elegido, y puede saltar al listado de un puente concreto

#### Scenario: Auto-refresh en vivo
- **WHEN** el usuario activa el auto-refresh en el listado
- **THEN** el listado se refresca periódicamente por polling y el indicador muestra la última actualización; apagado, no hay polling

### Requirement: Export, import y duplicar puentes
La consola SHALL exportar un puente (borrador o una versión publicada) como JSON descargable, importarlo creando un puente nuevo (borrador válido + publicación opcional) y duplicar un puente existente en un clic. El import SHALL validar contra el plano (validación del servidor como única fuente de verdad) y mostrar los errores de validación sobre el formulario.

#### Scenario: Exportar y compartir
- **WHEN** el usuario exporta la versión 3 de un puente
- **THEN** descarga un JSON con la definición congelada (sin ids de workspace) listo para importar en otro workspace

#### Scenario: Import con validación del servidor
- **WHEN** el usuario importa un JSON cuyo borrador tiene una ref rota
- **THEN** la consola crea el puente, ejecuta la validación del servidor y muestra los errores; no publica nada hasta que el borrador es válido
