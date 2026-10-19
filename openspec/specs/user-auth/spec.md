# user-auth Specification

## Purpose

Identidad por persona: registro, login y sesiones con token revocable, de modo que el acceso a un workspace se pueda conceder, limitar por rol y revocar sin rotar la API key compartida.

## Requirements

### Requirement: Registro de usuarios
El sistema SHALL exponer `POST /api/v1/auth/register` que crea un usuario con `email` único y `password`, almacenando únicamente el hash (argon2id). La respuesta SHALL devolver un token de sesión activo. El registro SHALL estar disponible sin autenticación previa: crear una cuenta no otorga acceso a ningún workspace.

#### Scenario: Registro exitoso
- **WHEN** se registra un email nuevo con una contraseña de al menos 10 caracteres
- **THEN** la respuesta es `201` con el usuario (`id`, `email`) y un token de sesión válido

#### Scenario: Email duplicado o contraseña corta
- **WHEN** se registra con un email ya existente o una contraseña menor a 10 caracteres
- **THEN** la respuesta es un Problem Details (409 / 422 respectivamente) sin revelar si el email existe en el caso de contraseña inválida

### Requirement: Login y sesiones con token revocable
El sistema SHALL exponer `POST /api/v1/auth/login` que valida email y contraseña y emite un token de sesión opaco (aleatorio de 256 bits, guardado solo como hash) con expiración configurable; `POST /api/v1/auth/logout` SHALL revocar la sesión del token presentado. El token SHALL usarse como `Bearer` en el plano admin y SHALL resolver al usuario y su rol en el workspace de la petición.

#### Scenario: Login válido emite sesión
- **WHEN** se autentican credenciales correctas
- **THEN** la respuesta es `200` con el token, su expiración y los workspaces accesibles con su rol

#### Scenario: Login inválido
- **WHEN** el email no existe o la contraseña no coincide
- **THEN** la respuesta es `401` con un mensaje uniforme que no distingue entre ambos casos

#### Scenario: Logout revoca la sesión
- **WHEN** se invoca logout con un token válido y después se reutiliza ese token
- **THEN** el logout responde `204`, y la siguiente petición con el mismo token es `401`

### Requirement: Identidad en la autenticación del plano admin
El plano admin SHALL aceptar dos identidades por `Bearer`: token de sesión de usuario o API key de workspace (compatibilidad). El mecanismo SHALL resolver un principal único con workspace y rol efectivo, y `GET /api/v1/whoami` SHALL revelar email (si es usuario), rol y workspace.

#### Scenario: whoami con token de usuario
- **WHEN** se consulta `whoami` con un token de sesión válido
- **THEN** la respuesta incluye el email, el rol efectivo en el workspace y la identidad de tipo usuario

#### Scenario: API key sigue siendo aceptada
- **WHEN** se llama a un endpoint del plano admin con una API key de workspace válida
- **THEN** la petición se resuelve como principal de tipo key con rol `owner`, sin cambiar ningún contrato de éxito existente
