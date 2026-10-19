# Design: add-mvp4-users-roles

## Context

MVP3 dejó la plataforma funcionalmente completa (eventos, dry-run, replay, bóveda versionada, mask, consola) pero el acceso es **por key compartida**: cualquiera con la key hace todo, no hay "quién" ni revocación por persona. El schema ya prepara la identidad desde MVP1: `users` (email único) y `workspace_members` (roles `owner`/`editor`/`runner`/`viewer`, PK compuesta). El plano admin resuelve hoy un `AuthedKey { workspace_id }` desde `api_keys.key_hash` (SHA-256 del crudo) en un solo middleware — los 100+ handlers dependen solo de ese `Extension`.

## Goals / Non-Goals

**Goals:**
- Login por persona (email + contraseña) con sesiones revocables, sin tocar el contrato de éxito de ningún endpoint existente.
- Enforcement de la matriz de roles sobre el plano admin, en un solo lugar.
- Gestión de membresía por el `owner` (añadir/cambiar rol/expulsar) con el último `owner` protegido.
- Consola con login real, UI por rol y página de miembros.

**Non-Goals:**
- Invitations por email con onboarding, SSO/OAuth, 2FA.
- Permisos por recurso individual (la unidad es el workspace).
- Cambiar `/x/{slug}` (invocación sin auth, spec vigente) ni los contratos de la API para las keys.
- Multi-workspace por sesión (v4: una sesión pertenece a un workspace; multi-workspace queda para después).

## Decisions

### D1 — Sesiones opacas con hash, no JWT
`user_sessions (id, user_id, workspace_id, token_hash UNIQUE, expires_at, revoked_at, last_used_at)`: token aleatorio de 256 bits, guardado solo como SHA-256 — exactamente el patrón de `api_keys` que ya funciona. Sin JWT: la revocación es un `UPDATE`, no hay firma que rotar ni dependencia nueva. El token nace ligado a un workspace (el login lo fija: implícito si el usuario tiene uno solo, elegido si tiene varios) y el rol se resuelve por petición (un cambio de rol es efectivo al instante).

### D2 — argon2id para contraseñas; SHA-256 solo para tokens
Las contraseñas se hashean con argon2id (crate `argon2`): SHA-256 es correcto para keys de alta entropía pero no para contraseñas humanas. Coste de verificación ~100 ms, aceptable para login y disuasorio de fuerza bruta. Mínimo de 10 caracteres validado al registrar.

### D3 — `Principal` en el middleware, handlers intactos
El middleware de auth resuelve `Principal::User { user_id, email }` o `Principal::ApiKey { key_id, name }`, carga la membresía y anota `AuthedKey { workspace_id, role, principal }` — los handlers siguen leyendo su `Extension<AuthedKey>` sin cambios de firma. La matriz de permisos vive en una tabla declarativa (segmento de ruta + método + rol mínimo) evaluada en un solo punto después de autenticar; test unitario propio de la matriz.

### D4 — Matriz de roles
`viewer`: GET del plano admin. `runner`: + disparo manual, replay, dry-run y `POST /events/{topic}`. `editor`: + CRUD de bloques, puentes, credenciales y publicación de versiones. `owner`: + membresías y API keys. `whoami`, `healthz` y docs: cualquier identidad. Las keys actúan como `owner` (backcompat total con CI y seed); `/x` no cambia.

### D5 — Migración 0006 sin destructivos
`ALTER TABLE users ADD COLUMN password_hash text` (nullable hasta que cada cuenta defina contraseña; login exige que exista) y `CREATE TABLE user_sessions` según D1. `api_keys` no cambia.

### D6 — Registro abierto, alta al workspace por el owner
`POST /auth/register` es público: crear cuenta no da acceso a nada. El `owner` añade por email **de un usuario existente** (404 claro si no existe, invitando a registrarse). Sin tokens de invitación en v4: menos superficie, el flujo completo ya es probado.

### D7 — Auditoría del disparador
`executions.trigger_principal jsonb NULL` (`{type: "user", email}` / `{type: "api_key", name}`), escrito al crear la ejecución en el plano http/admin; cron y el fan-out interno quedan como `system`. Expuesto en el detalle.

## Risks / Trade-offs

- **Fuerza bruta en login**: argon2 (~100 ms) frena; rate limiting explícito queda fuera de v4 (documentado).
- **Token en localStorage (XSS)**: mismo riesgo aceptado en MVP3; sin third-party JS el vector es mínimo, y el logout revoca en servidor.
- **Matriz declarativa vs. handlers**: centralizar en middleware puede denegar de más/menos → la matriz tiene test unitario exhaustivo y tests de integración por rol.
- **Rol resuelto por petición**: una consulta extra de membresía por petición autenticada de usuario — índice PK la hace barata; evita sesiones con permisos obsoletos.

## Migration Plan

0006 es aditiva (columna nullable + tabla nueva). Deploy: aplicar migración → desplegar binario. Rollback: el binario anterior ignora las columnas/tabla nuevas. Ningún dato se transforma.

## Open Questions

- ¿Expiración por defecto de sesión? Propuesta: 30 días, renovable por uso (se actualiza `last_used_at`; la expiración no se extiende en v4).
