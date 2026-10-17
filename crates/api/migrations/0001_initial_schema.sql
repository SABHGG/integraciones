-- Esquema completo desde el día 1 (design.md add-mvp1-core).
-- Las tablas de MVP2/MVP3 (webhook_deliveries, missed_triggers) nacen vacías.
-- IDs: UUID v7 generados por la aplicación; los defaults son fallback v4.

BEGIN;

-- ── Identidad / tenancy ────────────────────────────────────────────────
CREATE TABLE users (
    id         uuid PRIMARY KEY,
    email      text NOT NULL UNIQUE,
    name       text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspaces (
    id         uuid PRIMARY KEY,
    name       text NOT NULL,
    slug       text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspace_members (
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role         text NOT NULL CHECK (role IN ('owner', 'editor', 'runner', 'viewer')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);

-- En v1 la auth es una API key global (Bearer). workspace_id nullable para
-- cuando las keys sean por workspace.
CREATE TABLE api_keys (
    id           uuid PRIMARY KEY,
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    name         text NOT NULL,
    key_hash     text NOT NULL UNIQUE,  -- SHA-256 del key crudo; el crudo nunca se guarda
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at   timestamptz
);

-- ── Biblioteca de bloques ─────────────────────────────────────────────
CREATE TABLE blocks (
    id             uuid PRIMARY KEY,
    workspace_id   uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug           text NOT NULL,
    name           text NOT NULL,
    block_type     text NOT NULL CHECK (block_type IN ('http', 'custom_logic')),
    category       text NOT NULL DEFAULT 'general',
    description    text NOT NULL DEFAULT '',
    icon           text NOT NULL DEFAULT 'box',
    latest_version integer,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, slug)
);

-- code: JS transpilado con swc al publicar; NULL para bloques http built-in.
CREATE TABLE block_versions (
    id            uuid PRIMARY KEY,
    block_id      uuid NOT NULL REFERENCES blocks(id) ON DELETE CASCADE,
    version       integer NOT NULL,
    code          text,
    config_schema jsonb NOT NULL DEFAULT '{}'::jsonb,
    input_schema  jsonb NOT NULL DEFAULT '{}'::jsonb,
    output_schema jsonb NOT NULL DEFAULT '{}'::jsonb,
    ui            jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (block_id, version)
);

-- ── Puentes ───────────────────────────────────────────────────────────
CREATE TABLE bridges (
    id             uuid PRIMARY KEY,
    workspace_id   uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    slug           text NOT NULL,
    name           text NOT NULL,
    description    text NOT NULL DEFAULT '',
    latest_version integer,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, slug)
);

-- definition (JSONB): bloques instanciados + inputs + configs + trigger +
-- input_schema. El ejecutor lo lee como unidad atómica.
CREATE TABLE bridge_versions (
    id         uuid PRIMARY KEY,
    bridge_id  uuid NOT NULL REFERENCES bridges(id) ON DELETE CASCADE,
    version    integer NOT NULL,
    definition jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (bridge_id, version)
);

-- ── Bóveda de credenciales (compartidas por workspace) ────────────────
-- secret_encrypted: AES-256-GCM con clave de entorno; el valor nunca sale por
-- la API (write-only) y nunca entra al contexto ni al IO de ejecuciones.
CREATE TABLE credentials (
    id               uuid PRIMARY KEY,
    workspace_id     uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name             text NOT NULL,
    secret_encrypted bytea NOT NULL,
    description      text NOT NULL DEFAULT '',
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name)
);

-- ── Ejecuciones (la tabla ES la cola: pending + run_at + SKIP LOCKED) ─
CREATE TABLE executions (
    id                 uuid PRIMARY KEY,  -- UUID v7: sirve de executionId ordenable
    workspace_id       uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    bridge_id          uuid NOT NULL REFERENCES bridges(id) ON DELETE CASCADE,
    bridge_version_id  uuid NOT NULL REFERENCES bridge_versions(id),
    trigger_type       text NOT NULL CHECK (trigger_type IN ('http', 'cron', 'event', 'manual')),
    trigger_data       jsonb NOT NULL DEFAULT '{}'::jsonb,
    status             text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    idempotency_key    text,
    webhook_url        text,              -- MVP2
    error              jsonb,             -- {code, message, block?, details?}
    run_at             timestamptz NOT NULL DEFAULT now(),
    started_at         timestamptz,
    finished_at        timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);

-- Cola: solo pendientes, ordenadas por run_at
CREATE INDEX idx_executions_pending ON executions (run_at) WHERE status = 'pending';
-- Paginación por cursor en listados
CREATE INDEX idx_executions_bridge_created ON executions (bridge_id, created_at DESC);
-- Idempotencia opcional del disparo HTTP: repetida → devuelve la ejecución previa
CREATE UNIQUE INDEX uq_executions_idempotency
    ON executions (workspace_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- ── Pasos de ejecución: IO completo por bloque ────────────────────────
CREATE TABLE execution_steps (
    id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    execution_id      uuid NOT NULL REFERENCES executions(id) ON DELETE CASCADE,
    seq               integer NOT NULL,
    block_instance_id text NOT NULL,      -- id de la instancia dentro de la definición
    block_name        text NOT NULL,
    status            text NOT NULL CHECK (status IN ('succeeded', 'failed')),
    input             jsonb NOT NULL,
    output            jsonb,
    error             jsonb,
    duration_ms       integer,
    created_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (execution_id, seq)
);

-- ── MVP2: webhook de resultado ────────────────────────────────────────
CREATE TABLE webhook_deliveries (
    id               uuid PRIMARY KEY,
    execution_id     uuid NOT NULL REFERENCES executions(id) ON DELETE CASCADE,
    url              text NOT NULL,
    status           text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'failed')),
    attempts         integer NOT NULL DEFAULT 0,
    response_status  integer,
    error            text,
    next_retry_at    timestamptz,
    last_attempt_at  timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now()
);

-- ── MVP2: disparos perdidos (solapamiento o caída) ────────────────────
CREATE TABLE missed_triggers (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    bridge_id    uuid NOT NULL REFERENCES bridges(id) ON DELETE CASCADE,
    scheduled_at timestamptz NOT NULL,
    reason       text NOT NULL CHECK (reason IN ('overlap', 'downtime')),
    created_at   timestamptz NOT NULL DEFAULT now()
);

COMMIT;
