# Proposal

## Why

El MVP3 cerró el roadmap funcional, pero dejó la seguridad del acceso **por identidad** aplazada a sabiendas: la consola usa una API key de workspace compartida en el navegador (decisión reconocida en el design de MVP3 como "el riesgo aceptado"). Hoy cualquier persona con la key puede hacer **todo** (crear, borrar, publicar, rotar secretos); no hay forma de saber **quién** hizo qué, ni de revocar el acceso de una persona sin rotar la key de todo el workspace.

El schema ya está preparado desde el MVP1: `users` (email único) y `workspace_members` (roles `owner`/`editor`/`runner`/`viewer`) existen y están sin usar. Este change activa esa base y convierte la plataforma en multiusuario real sin romper la automatización por API key.

## What Changes

- **Sesiones de usuario**: registro (`email` + `password` con hash argon2id), login y logout con tokens de sesión opacos (hash SHA-256 en `user_sessions`, expiración y revocación — mismo patrón que `api_keys`). El `Bearer` acepta ahora dos identidades: token de usuario o API key de workspace.
- **Roles enforcement**: matriz de permisos sobre el plano admin según el rol del miembro (`viewer` lee; `runner` ejecuta además; `editor` administra recursos; `owner` administra personas y keys). Los handlers existentes no cambian de firma: el middleware resuelve el `Principal` y anota el rol.
- **Gestión de miembros**: el `owner` lista, añade (por email de usuario existente), cambia rol y expulsa miembros del workspace.
- **Compatibilidad**: las API keys existentes actúan como `owner` (backcompat para CI/máquinas); siguen creándose y revocándose igual. El plano de invocación `/x/{slug}` no cambia (sin auth, spec vigente).
- **Consola**: login con email y contraseña (en vez de pegar la key), UI adaptada al rol (acciones ocultas/deshabilitadas para `viewer`/`runner`), página de miembros para el `owner` y cierre de sesión que revoca el token.
- **Auditoría**: `whoami` revela identidad (email, rol) y la ejecución queda ligada a su disparador.

## Capabilities

### New Capabilities

- `user-auth`: registro, login, sesiones con expiración/revocación y emisión del principal de usuario.
- `workspace-roles`: matriz de permisos por rol sobre el plano admin y gestión de miembros del workspace.

### Modified Capabilities

- `web-console`: el login pasa a email+contraseña con token de sesión; la UI se adapta al rol y añade la página de miembros para el `owner`.

## Impact

- **Migración `0006`**: `users.password_hash`, tabla `user_sessions` (hash del token, expiración, revocación). Sin cambios destructivos: `api_keys` queda igual.
- **Código**: `crates/api/src/auth.rs` (middleware con `Principal` + rol), `crate argon2` nueva en el workspace, endpoints `auth` y `members`, `executions` anota el disparador. La consola (`console/src`) cambia login/sesión y añade la pantalla de miembros.
- **Compatibilidad**: ningún endpoint existente cambia de contrato de éxito; los errores nuevos son 401/403 Problem Details. Los scripts de CI/seed (`demo-key-1`) siguen funcionando como `owner`.
- **Fuera de alcance**: invitations por email con onboarding, SSO/OAuth, permisos por recurso individual, rotación forzada de passwords.
