-- MVP4 (usuarios y roles): identidad por persona sobre la base de MVP1.
-- users.password_hash es nullable: la cuenta existe desde el registro; el
-- login exige que tenga contraseña definida.
ALTER TABLE users ADD COLUMN password_hash text;

-- Sesiones opacas por workspace: del token crudo solo se guarda su SHA-256
-- (mismo patrón que api_keys). workspace_id nullable: una sesión sin
-- workspace exige el header X-Workspace en cada petición (multi-workspace).
CREATE TABLE user_sessions (
    id           uuid PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    token_hash   text NOT NULL UNIQUE,
    expires_at   timestamptz NOT NULL,
    revoked_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz
);

CREATE INDEX idx_user_sessions_user ON user_sessions(user_id);
