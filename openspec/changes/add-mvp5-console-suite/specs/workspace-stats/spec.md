# Spec Delta: workspace-stats

## Purpose

Métricas agregadas de ejecución del workspace (solo lectura) para que el dashboard muestre salud y tendencia sin descargar el listado completo.

## ADDED Requirements

### Requirement: Métricas agregadas del workspace
El sistema SHALL exponer `GET /api/v1/stats` (plano admin, `editor` no requerido: cualquier identidad del workspace) que agregue sobre `executions` del workspace para un rango `from`/`to` (RFC 3339, por defecto últimos 30 días): total de ejecuciones, conteo por estado, serie diaria (día, total, exitosas, fallidas), duración media de las exitosas (ms) y desglose por puente (slug, total, exitosas, fallidas, duración media). La respuesta SHALL ser una sola consulta agregada (sin N+1) y SHALL paginar el desglose por puente por cursor si supera el límite.

#### Scenario: Métricas de un rango
- **WHEN** se solicita `GET /stats?from=…&to=…` de un workspace con ejecuciones
- **THEN** la respuesta incluye el total, el conteo por estado, la serie diaria y la duración media de exitosas del rango

#### Scenario: Desglose por puente
- **WHEN** la respuesta incluye el desglose por puente
- **THEN** cada entrada identifica el puente (id y slug) con total, exitosas, fallidas y duración media de ese rango

#### Scenario: Rango inválido
- **WHEN** `from` o `to` no son RFC 3339 válidos o `from` > `to`
- **THEN** la respuesta es `400` Problem Details sin ejecutar la agregación
