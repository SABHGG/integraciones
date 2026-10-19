# Spec Delta: workspace-stats

## Purpose

Métricas agregadas de ejecución del workspace (solo lectura) para que el dashboard muestre salud y tendencia sin descargar el listado completo.

## ADDED Requirements

### Requirement: Métricas agregadas del workspace
El sistema SHALL exponer `GET /api/v1/stats` (plano admin, cualquier identidad del workspace) que agregue sobre `executions` para un rango `from`/`to` (RFC 3339, por defecto últimos 30 días): total de ejecuciones, conteo por estado, serie diaria y duración media de las exitosas (ms). La respuesta SHALL salir de una sola consulta agregada (sin N+1), y los días sin ejecuciones SHALL poder rellenarlos los consumidores del rango.

#### Scenario: Métricas de un rango
- **WHEN** se solicita `GET /stats?from=…&to=…` de un workspace con ejecuciones
- **THEN** la respuesta incluye el total, el conteo por estado, la serie diaria y la duración media de exitosas del rango

### Requirement: Desglose por puente
La respuesta SHALL incluir el desglose por puente del mismo rango (id y slug, total, exitosas, fallidas, duración media), paginado por cursor cuando supere el límite de página.

#### Scenario: Desglose por puente
- **WHEN** la respuesta incluye el desglose por puente
- **THEN** cada entrada identifica el puente con total, exitosas, fallidas y duración media de ese rango

### Requirement: Validación del rango
El rango SHALL validarse antes de agregar: `from`/`to` deben ser RFC 3339 válidos, `from` ≤ `to` y el rango no MAYOR a 90 días; en caso contrario `400` Problem Details sin ejecutar la agregación.

#### Scenario: Rango inválido
- **WHEN** `from` o `to` no son RFC 3339 válidos, `from` > `to` o el rango supera 90 días
- **THEN** la respuesta es `400` Problem Details sin ejecutar la agregación
