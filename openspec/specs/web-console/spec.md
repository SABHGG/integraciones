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
