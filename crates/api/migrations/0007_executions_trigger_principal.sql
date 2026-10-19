-- MVP4 (workspace-roles, task 3.3): auditoría del disparador. Qué principal
-- creó la ejecución: {"type":"user","email":…} | {"type":"api_key","name":…}
-- | {"type":"system"} (cron, fan-out interno por emit). NULL = /x sin
-- identidad (el plano de invocación no autentica).
ALTER TABLE executions ADD COLUMN trigger_principal jsonb;
