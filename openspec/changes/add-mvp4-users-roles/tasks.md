# Tasks: add-mvp4-users-roles

## 1. Fase 1 — Sesiones de usuario

- [x] 1.1 Migración `0006`: `users.password_hash` (nullable) + `user_sessions` (token hash único, workspace, expiración, revocación); aplicable sobre el schema existente; tests RED/GREEN
- [x] 1.2 `POST /auth/register` (argon2id, email único, contraseña ≥ 10) y `POST /auth/login` (401 uniforme, sesión por workspace: implícito con uno, elegido con varios); tests
- [x] 1.3 `POST /auth/logout` (revoca) + expiración 30 días; `whoami` revela email/rol/tipo de principal; tests
- [x] 1.4 Middleware `Principal`: token de usuario o API key → `AuthedKey { workspace, role, principal }` con rol resuelto por petición; los handlers existentes intactos; tests

## 2. Fase 2 — Matriz de roles

- [ ] 2.1 Matriz declarativa (segmento + método → rol mínimo) evaluada en un punto tras autenticar: `403` con rol requerido, `401` sin identidad; test unitario exhaustivo de la matriz
- [ ] 2.2 API keys existentes = `owner` (backcompat CI/seed); tests de que ningún contrato de éxito cambia con key
- [ ] 2.3 Tests de integración por rol sobre recursos reales: viewer no edita, runner dispara pero no crea, editor administra sin tocar membresía, owner todo

## 3. Fase 3 — Miembros

- [ ] 3.1 `GET /members`, `POST /members` (por email existente, 404 claro si no hay cuenta), `PATCH /members/{id}` (rol), `DELETE /members/{id}`; solo owner; tests
- [ ] 3.2 Protección del último owner (409 al degradar o expulsar); tests
- [ ] 3.3 Auditoría: `executions.trigger_principal` (migración 0006 aditiva) escrito en el plano http/admin; detalle lo expone; cron/fan-out interno = system; tests

## 4. Fase 4 — Consola

- [ ] 4.1 Login con email+contraseña (token de sesión en vez de key), logout que revoca en servidor; pantalla de elección de workspace si el login lo requiere; tests del flujo con el plano real
- [ ] 4.2 UI por rol: ocultar/deshabilitar acciones según rol del `whoami`; errores `403` del servidor visibles donde se intenten; tests de render por rol
- [ ] 4.3 Página de miembros (lista + añadir/cambiar rol/expulsar) con errores del servidor en línea; solo owner edita; tests

## 5. Fase 5 — Cierre

- [ ] 5.1 OpenAPI: auth (register/login/logout), members, roles en errores 403; validado
- [ ] 5.2 README: sección de usuarios, roles y login de la consola con ejemplos; límites (sin invitations, sin rate limit) documentados
- [ ] 5.3 E2E punta a punta: registro → owner añade → login por rol → editor publica → viewer denegado → replay audita email → expulsión revoca acceso; CI verde y archive
