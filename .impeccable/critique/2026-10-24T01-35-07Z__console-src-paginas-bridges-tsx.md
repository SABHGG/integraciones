---
target: editor de puentes (EditorPuente, console/src/paginas/bridges.tsx)
total_score: 25
max_score: 40
na_heuristics: 
p0_count: 2
p1_count: 2
target_identity: "file:/home/sergi/integraciones/console/src/paginas/bridges.tsx"
target_fingerprint: "sha256:a41fecc467dd83a94e4a1b7ff19dde3bf789fa0f5750a630e71749ef01787157"
target_path: /home/sergi/integraciones/console/src/paginas/bridges.tsx
timestamp: 2026-10-24T01-35-07Z
slug: console-src-paginas-bridges-tsx
---
# Critique — Editor de puentes (EditorPuente, console/src/paginas/bridges.tsx)

Method: dual-agent (A: gentle-ai-explore · B: gentle-ai-verify)

## Design Health Score

| # | Heurística | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibilidad del estado | 2 | Guardar/Validar/Publicar sin `procesando`; no hay dirty state |
| 2 | Lenguaje del usuario | 3 | "Input schema" habla de "trigger http" aunque el trigger sea cron/event/manual |
| 3 | Control y libertad | 2 | "Quitar" sin confirmación ni undo; reordenar de a uno |
| 4 | Consistencia | 3 | Publicar (irreversible) no usa el patrón de confirmación que ui.tsx documenta |
| 5 | Prevención de errores | 1 | Publicar sin guardar publica el borrador viejo; parseo silencioso; tipo huérfanos |
| 6 | Reconocimiento | 3 | datalist IANA y refs con fuente; hay que recordar nombres de instancia |
| 7 | Flexibilidad | 3 | Ctrl/Cmd+Espacio y JSON avanzado; faltan atajos de guardado y duplicar |
| 8 | Estética minimalista | 2 | Instancias siempre expandidas: muro con 5+ pasos |
| 9 | Recuperación de errores | 3 | Issues sobre el canvas (raíl/nudo rojo); parseo local no avisa |
| 10 | Ayuda | 3 | Ayudas en línea concretas; sin help contextual dentro del editor |
| **Total** | | **25/40** | **Acceptable — mejoras significativas antes de usuarios reales** |

## Design Specificity Verdict

**Diseñado para este producto, con zona gris.** El rastro está literalizado (raíl + nudos que se tiñen al fallar), "Ensayo sin efectos" habla el idioma del dominio, la mono respeta "solo valores de máquina" y `contextoDeRefs(indice)` —solo salidas de bloques anteriores— es imposible en un producto genérico. Zona gris: los paneles Trigger/Input schema/Cadena son títulos de 14px sobre Panel blancos; sin los refs `$.` sería cualquier admin CRUD.

**Deterministic scan (B):** `impeccable detect --json` sobre los 4 archivos del editor → **0 hallazgos** (exit 0). Sensibilidad validada aparte: el detector marcó 2 hallazgos sobre un archivo sintético con tells clásicos (AI beige, eyebrow tracking-widest, pulso emerald), así que el cero es una señal real: **no hay slop**. Los problemas del editor son estructurales (flujo, prevención, densidad), no anti-patrones estéticos. browser visualization skipped: no browser automation available.

## Overall Impression

El editor tiene dos ideas propias de calidad (el raíl-erre-map y el autocompletado contextual) y una base sin slop; lo que le pesa es el contrato de guardado/publicación (tres acciones desconectadas que prometen más de lo que hacen) y la densidad de las tarjetas siempre expandidas. La mayor oportunidad: que Publicar signifique lo que el usuario cree que significa.

## What's Working

1. **El raíl del rastro como mapa de errores** — metáfora del producto hecha interfaz; dónde falla la cadena se ve sin leer mensajes.
2. **Autocompletado de refs contextual por posición** — resuelve el problema más duro del dominio en el punto exacto de escritura (interpolar/mapear, badges de fuente).
3. **Errores del servidor pintados sobre el canvas** — sin la lista genérica de errores que no corresponden a nada.

## Priority Issues

1. **[P0] Publicar versión publica un borrador que puede no ser lo que ves.** Editar + Publicar sin "Guardar borrador" publica el borrador guardado anterior y descarta la edición en silencio; sin confirmación para una acción irreversible; sin `procesando` (doble clic = versión duplicada). Fix: detectar dirty → PATCH + validate + confirmación con resumen; botón con `procesando`. Comando: `harden`.
2. **[P0] Parseo JSON silencioso** en las textareas `when`, `input` de instancia y pestañas JSON avanzado: catch vacíos dejan el estado obsoleto sin feedback ("lo que veo no es lo que hay"). Fix: patrón de `EditorJson` (borde de error + nota "no se guarda hasta que parsee") en todos los editores texto-a-JSON. Comando: `clarify`.
3. **[P1] "Quitar" instancia: destrucción instantánea sin red** de una tarjeta con config/input/when trabajados. Fix: toast con "Deshacer" (memoria en sesión) o confirmación en dos pasos para instancias con contenido. Comando: `harden`.
4. **[P1] Tarjetas siempre expandidas → muro con cadenas largas** (~50 controles con 8 pasos). Fix: fila-resumen colapsable (nº, nombre, badge when, nudo de error); expandido solo el último/errores. Comando: `distill`.
5. **[P2] "Añadir instancia": sopa de botones `+ slug` sin búsqueda** (N sin límite, rompe ≤4 opciones). Fix: combobox con búsqueda agrupada por blockType. Comando: `optimize`.

## Persona Red Flags

- **Alex (power user):** "Validar" valida el borrador *guardado*, no lo editado (puede validar basura vieja); reordenar de a uno en cadenas largas; sin atajos de guardado/ensayo.
- **Sam (teclado/lector):** listbox de refs sin `aria-activedescendant`; ↑/↓/Quitar sin `aria-label`/`title` en instancias; `Aviso` sin `role="status"`/`aria-live` (los mensajes no se anuncian). Lo bueno: focus-visible consistente en todo el kit.
- **Riley (stress tester):** doble clic en Publicar → versiones duplicadas; cambiar tipo http→cron deja method/path huérfanos; JSON roto asentido en silencio.

## Minor Observations

- Tono de `Aviso` por string (`startsWith("error")`): frágil.
- Panel Input schema siempre visible aunque el trigger no sea http.
- Botón "Ir a Bloques" embebido en prosa del empty state.
- En móvil el nombre del puente desaparece (`hidden lg:inline`).
- El patrón de pestañas guiado↔avanzado está duplicado (trigger y schema) — candidato a componente compartido.

## Questions to Consider

1. ¿Por qué Guardar/Validar/Publicar son tres contratos separados cuando son una sola cosa con grados de compromiso?
2. Si el panel Input schema solo existiera cuando el trigger es http, ¿qué aprendería el usuario del modelo mental?
3. Si un puente real llega a 20 pasos, ¿sobrevive la tarjeta siempre expandida, o el raíl muere ahogado en controles?
