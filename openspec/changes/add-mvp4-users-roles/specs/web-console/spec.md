# Spec Delta: web-console

## Purpose

La consola pasa de la API key compartida a identidad por persona: login con email y contraseña, UI adaptada al rol y gestión de miembros para el `owner` — sin endpoints de negocio nuevos.

## MODIFIED Requirements

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

## ADDED Requirements

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
