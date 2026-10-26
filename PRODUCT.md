# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Equipo técnico interno (equipo/organización propietaria del despliegue) que conecta APIs internas y de terceros. Situación típica: automatizar un flujo (p. ej. consultar una API al recibir un webhook, correr trabajos programados, encadenar llamadas con transformación) desde la consola o su API, con el taller en `docs/` como ruta de aprendizaje. Roles confirmados por la plataforma: `viewer` (lectura), `runner` (dispara/ensaya), `editor` (administra recursos), `owner` (gestiona miembros).

## Product Purpose

Plataforma de integraciones auto-hospedada: puentes (cadenas lineales de bloques) disparables por HTTP (ANY `/x/{slug}`), cron (5 campos + zona IANA) y eventos, con consola web para construirlos (trigger guiado, inputSchema, refs `$.`, dry-run «Ensayo sin efectos») y API pública. El éxito es una integración creada → validada → publicada (versión inmutable) → ejecutada con trace completo, sin saltar a producción rota.

## Positioning

Dos cosas que un SaaS vecino no copiaría tal cual, decididas por el propietario:

1. **Self-hosted / soberanía**: BD propia, bóveda de secretos write-only propia, triggers cron/evento y entrega de webhooks sin SaaS externo.
2. **Integraciones como código**: puentes y bloques con versiones inmutables, diff y rollback; el borrador se valida antes de congelarse.

## Operating Context

- Consola y API viven en el mismo binario Rust (`crates/`); front-end Preact + Tailwind v4 en `console/` servido por el binario.
- El proyecto se gestiona con OpenSpec (`openspec/`, specs en español, cambio → validate → apply → archive); la CI exige formato/clippy/tests/e2e.
- Toda la documentación de usuario está en español: guías (`docs/guia-consola.md`, `docs/guia-bloques.md`, `docs/guia-puentes.md`, `docs/errores-comunes.md`) y talleres paso a paso (`docs/talleres.md`).

## Capabilities and Constraints

- Disparadores: `http` (ANY `/x/{path}`), `cron` (5 campos, timezone IANA), `event` (topics `a-z0-9.-`), `manual` (datos de prueba → `$.trigger`).
- Refs `$.` con raíces `$.trigger`/`$.now` y salidas nombradas de bloques previos; interpolación `{$.ref}` en config, mapeo directo en input.
- Bloques: built-in `http` y `custom_logic` (TypeScript → sandbox JS con `ctx.*`); formularios generados desde metadatos `ui` (contrato cerrado: title/order/widget/help).
- Publicación inmutable: draft → validate (server es autoridad) → publicar versión; publish irreversible (la consola pide confirmación con resumen).
- Secretos: credenciales con versiones, la consola nunca lee el valor (bóveda write-only).
- Sin cambios de backend aprobados para trabajo UI: la consola no duplica contratos, los refleja.

## Brand Commitments

- Sistema visual **«El rastro»** (nombre documentado en specs y commits; es vinculante).
- Idioma de UI: español (con términos de dominio en mono: slugs, refs `$.`, estados de ejecución).
- La mono se reserva para valores de máquina; el color significa estado (ok/mal/info/aviso), no decoración.

## Evidence on Hand

- Guías de usuario y talleres verificados contra el código: `docs/`, `README.md`.
- Specs OpenSpec de las 18 capabilities en `openspec/specs/`.
- Tests Rust (unit + e2e) y consola (`npm run test`); no hay testimonios, prensa ni benchmarks — prohibido fabricarlos.

## Product Principles

1. **El servidor valida; la consola guía.** Una sola fuente de verdad por contrato (validate_trigger, ui, roles), reflejada — nunca duplicada — en la UI.
2. **Nada toca producción rota.** Dry-run, validate y confirmación explícita antes de congelar; lo irreversible se anuncia, no se presume.
3. **El rastro se ve.** La cadena es el rastro: raíl, nudos de estado, trace paso a paso; el error señala dónde caer, no solo que caíste.
4. **Guardas con curva amable.** Autocompletado y formularios guiados para no teclear JSON, con JSON avanzado respetado como salida de emergencia.

## Accessibility & Inclusion

Objetivo declarado: **WCAG 2.1 AA** (contraste 4.5:1, foco visible en todo el kit, navegación por teclado completa, estados anunciados con `aria-live`). Heredado del critique: labels en acciones iconográficas y `aria-activedescendant` en autocompletados son parte de la barra.
