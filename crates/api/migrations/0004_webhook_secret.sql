-- MVP2 (change add-mvp2-async-cron-conditionals): secreto HMAC del webhook.
--
-- Un secreto de 256 bits por puente, generado por el sistema al publicar la
-- primera versión con webhook y estable entre versiones (regenerarlo rompería
-- la verificación en los consumidores). Se guarda CIFRADO en reposo con la
-- misma envolvente AES-256-GCM de la bóveda (`vault`: nonce(12) || AEAD) y
-- solo sale por el plano de administración (GET /bridges/{id}/webhook-secret).
-- `NULL` = puente sin webhook (o anterior a MVP2).

ALTER TABLE bridges ADD COLUMN webhook_secret bytea;
