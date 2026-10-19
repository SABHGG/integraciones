# web-console Specification

## Purpose
Primera UI propia de la plataforma: una consola web servida por el mismo binario que convierte el plano de administración en un producto utilizable sin curl — dashboard de ejecuciones con trace, gestión visual de puentes y bloques, formularios generados por los metadatos `ui` y editor de código con los errores del validador en línea.

## Requirements

### Requirement: Consola servida por el binario
La consola SHALL seguir siendo una SPA de assets estáticos servida por el mismo binario bajo `/console` que consume exclusivamente el plano de administración existente. El acceso SHALL autenticarse primariamente con **email y contraseña** (token de sesión del plano `auth`); la API key de workspace SHALL seguir aceptada como acceso alternativo de automatización/demo (actúa como `owner`). No SHALL añadirse endpoints de negocio nuevos para la consola.

#### Scenario: Login con email y contraseña
- **WHEN** el usuario introduce credenciales válidas en la pantalla de acceso
- **THEN** la consola obtiene un token de sesión vía el plano `auth` y lo usa como Bearer en todas sus llamadas

#### Scenario: Login con API key
- **WHEN** el usuario introduce una API key de workspace válida en el acceso alternativo
- **THEN** la consola carga el dashboard usando esa key como Bearer (principal de tipo key, rol `owner`)

#### Scenario: Key inválida o sin workspace
- **WHEN** se introduce una API key inexistente o sin workspace asociado
- **THEN** el acceso se rechaza con el error del plano admin y no se carga ningún dato

#### Scenario: Credenciales inválidas
- **WHEN** se introduce un email o contraseña incorrectos
- **THEN** el error mostrado es el `401` uniforme del plano admin y no se carga ningún dato

#### Scenario: Cierre de sesión revoca el token
- **WHEN** el usuario pulsa salir con sesión de usuario
- **THEN** la consola invoca el logout del plano (revocando la sesión en el servidor) y borra el token local

### Requirement: Dashboard de ejecuciones con trace
La consola SHALL presentar el listado de ejecuciones con filtros por puente, estado y rango de fechas (paginación por cursor), el detalle de cada ejecución y un visor de trace paso a paso — incluyendo los pasos `skipped` — con el IO (ya enmascarado) y el punto exacto del fallo.

#### Scenario: Diagnosticar un fallo hasta el bloque exacto
- **WHEN** el usuario abre una ejecución failed desde el dashboard
- **THEN** el visor señala el paso que falló con su error y permite inspeccionar el input que recibió

#### Scenario: Los pasos condicionales se ven como skipped
- **WHEN** el trace de un puente con `switch` muestra rutas no tomadas
- **THEN** el visor las presenta como pasos `skipped` en su posición de la cadena

### Requirement: Formularios generados por metadatos ui
Los formularios de configuración de instancias SHALL generarse automáticamente desde el campo `ui` de la versión del bloque (tipos, etiquetas, valores por defecto y validación de cliente); un bloque sin metadatos `ui` SHALL recibir un formulario JSON genérico.

#### Scenario: Formulario derivado de los metadatos
- **WHEN** el usuario añade una instancia del bloque `http-get` a un puente
- **THEN** el formulario muestra sus campos declarados en `ui` (url, headers, timeout…) con su validación

#### Scenario: Bloque sin ui metadatos
- **WHEN** un bloque `custom_logic` se publica sin campo `ui`
- **THEN** su configuración se edita como JSON libre con validación contra inputSchema al guardar

### Requirement: Editores con validación en línea
La consola SHALL incluir un editor de código (TS/JS) para bloques `custom_logic` y un editor visual de la cadena del puente (orden lineal, `when` y rutas de `switch`): los errores de publicación del servidor (transpile, schemas, rutas hacia atrás, refs imposibles) SHALL mostrarse sobre el editor antes de permitir publicar.

#### Scenario: Error de TypeScript visible al publicar
- **WHEN** el usuario publica un bloque con TypeScript que no transpila
- **THEN** la consola muestra el error del validador junto a la línea correspondiente y no publica

#### Scenario: Ruta de switch inválida marcada en el editor visual
- **WHEN** una ruta del `switch` apunta a una instancia anterior en la cadena
- **THEN** el editor visual la marca como error de validación y la publicación queda bloqueada

### Requirement: UI adaptada al rol
La consola SHALL ocultar o deshabilitar las acciones que el rol del usuario no puede ejecutar (según la matriz del plano): `viewer` navega en lectura; `runner` además dispara, replayea y ensaya; `editor` administra recursos; `owner` además gestiona miembros. Las acciones denegadas por el servidor SHALL mostrarse con el `403` recibido (una sola fuente de verdad: la consola no duplica la matriz).

#### Scenario: Viewer en modo lectura
- **WHEN** entra un `viewer` al editor de puentes
- **THEN** ve la cadena y el trace sin botones de guardar, publicar ni eliminar

#### Scenario: Owner gestiona la membresía
- **WHEN** entra un `owner` a la página de miembros
- **THEN** ve la lista con emails y roles, y puede añadir, cambiar rol y expulsar con los errores del servidor en línea

### Requirement: Página de miembros
La consola SHALL incluir una vista de miembros del workspace (visible íntegra para `owner`, de solo lectura para el resto): lista con email, rol y fecha de alta, y las acciones de la gestión de miembros del plano admin con sus errores visibles.

#### Scenario: Miembro no owner ve la lista sin acciones
- **WHEN** un `editor` abre la página de miembros
- **THEN** ve la lista completa sin controles de edición

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
