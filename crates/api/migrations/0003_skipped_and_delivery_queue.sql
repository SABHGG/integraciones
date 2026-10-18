-- MVP2 (change add-mvp2-async-cron-conditionals):
-- enrutado condicional y cola de entregas webhook.
--
-- 1) `execution_steps.status` pasa a admitir `skipped`: los pasos no ejecutados
--    por enrutado condicional (`when` falso o ruta de `switch` no tomada) se
--    registran en su `seq` original sin input ejecutado, salida ni duración.
--    La columna `input` es NOT NULL: para un paso skipped se persiste `'{}'`
--    (objeto vacío) como marcador de "sin input ejecutado".
--
-- 2) Índice parcial para la cola de entregas del job de webhooks: solo las
--    entregas pendientes, ordenadas por su próxima fecha de reintento
--    (mismo patrón que `idx_executions_pending` sobre la cola de ejecuciones).

ALTER TABLE execution_steps DROP CONSTRAINT execution_steps_status_check;
ALTER TABLE execution_steps ADD CONSTRAINT execution_steps_status_check
    CHECK (status IN ('succeeded', 'failed', 'skipped'));

CREATE INDEX idx_webhook_deliveries_pending
    ON webhook_deliveries (next_retry_at) WHERE status = 'pending';
