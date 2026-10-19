-- MVP3 (change add-mvp3-events-observability-ui): bóveda versionada (D4).
--
-- Cada escritura del valor crea una versión INMUTABLE en credential_versions
-- (cifrada con la misma envolvente AES-256-GCM de la bóveda) y `credentials`
-- pasa a apuntar a la versión vigente con `current_version_id`. La migración
-- copia el secreto vigente de `credentials.secret_encrypted` como versión 1
-- (conservadora: nada se pierde y el runtime cambia de fuente sin editar
-- definiciones). La columna legada `credentials.secret_encrypted` deja de ser
-- la fuente de verdad del runtime (se conserva por seguridad de rollback).

CREATE TABLE credential_versions (
    id               uuid PRIMARY KEY,
    credential_id    uuid NOT NULL REFERENCES credentials(id) ON DELETE CASCADE,
    secret_encrypted bytea NOT NULL,
    rotated_at       timestamptz NOT NULL DEFAULT now(),
    created_at       timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_credential_versions_credencial
    ON credential_versions (credential_id, rotated_at);

ALTER TABLE credentials ADD COLUMN current_version_id uuid REFERENCES credential_versions(id);

-- Copia conservadora: el secreto vigente pasa a ser la versión 1 de cada
-- credencial existente.
INSERT INTO credential_versions (id, credential_id, secret_encrypted, rotated_at, created_at)
SELECT gen_random_uuid(), id, secret_encrypted, updated_at, created_at
FROM credentials;

UPDATE credentials c
SET current_version_id = cv.id
FROM credential_versions cv
WHERE cv.credential_id = c.id;

-- MVP3: auditoría de credenciales usadas por ejecución (pares
-- {credential_id, version_id} resueltos durante sus pasos, sin valores).
ALTER TABLE executions ADD COLUMN credentials_used jsonb;
