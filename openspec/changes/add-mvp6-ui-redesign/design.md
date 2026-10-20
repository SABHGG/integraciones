# Design

## Contexto

La consola (Preact + Vite, `console/`) tiene cinco hitos de funcionalidad sobre `styles.css` artesanal: variables sueltas, tarjetas y tablas mínimas, sin sistema. Este change reconstruye la capa visual con una dirección elegida y congelada, sin tocar endpoints de negocio ni la matriz de roles.

## Decisiones

### D1 — Dirección congelada: "El rastro" (target en `target/`)

Ronda de conceptos A/B/C/D presentada al propietario con capturas reales (Playwright, 1440×900); elegido D. El target congelado vive en `target/d-el-rastro.png` (1440×900, verificado) junto a `d-el-rastro.html` reproducible y las fuentes. Es el contrato espacial del rediseño: la página de Ejecuciones implementada DEBE renderizar 1:1 contra él en la primera verificación; las demás páginas heredan la identidad (tokens, shell, tipografía, componentes) sin nueva aprobación.

Limitación documentada: el modelo de esta sesión no tiene visión ni hay generador de imágenes configurado; la inspección visual del target y de los renders finales es humana. La verificación automatizada disponible es estructural (Playwright: desbordes, dimensiones, fuentes cargadas) y de contraste (cálculo WCAG sobre los tokens).

### D2 — Tokens con reglas

| Token | Valor | Uso |
|---|---|---|
| `--papel` | `#F5F6F3` | fondo (frío, no crema) |
| `--tinta` | `#14352D` | texto, sidebar, botón primario, gráficos de serie |
| `--tinta-suave` | `#5B7169` | texto secundario, etiquetas de columna |
| `--tinta-tenue` | `#64706B` | metadato terciario, ejes |
| `--superficie` | `#FFFFFF` | paneles y tablas |
| `--borde` / `--borde-fuerte` | `#E1E5DF` / `#CDD3CC` | reglas y controles |
| estado ok | `#1E7A4E` | succeeded |
| estado mal | `#B3402E` | failed |
| estado aviso | `#855D0E` | pending/skipped |
| estado info | `#2B6CB0` | running |

Reglas del sistema (no negociables): **el color solo significa datos** — verde/ladrillo/ámbar/azul expresan estado de ejecución o paso, nunca decoración; el accionar se comunica con tinta sólida. **Mono solo para valores de máquina** — slugs, triggers, estados literales, duraciones, JSON, códigos de error; cero etiquetas o cabeceras en mono. **Una audacia** — la sidebar de tinta; lo demás callado (radios moderados 6-10px, sin sombras decorativas, una sola elevación). **Sin movimiento automático** — cero animaciones al cargar; el movimiento solo confirma acciones, y cae con `prefers-reduced-motion`.

### D3 — Tipografías autoalojadas

Schibsted Grotesk (400/500/600/700) para interfaz y Spline Sans Mono (400/500/600) para valores de máquina: 4 woff2 ~122 KB en `console/public/fonts/`, `@font-face` con `font-display: swap`, fallback `system-ui` / `monospace`. Sin CDN: la consola embebida funciona offline.

### D4 — Tailwind v4 como base

`tailwindcss@^4` + `@tailwindcss/vite` como devDependencies; tokens en `@theme` inside CSS; utilidades en las páginas. Sin runtime en el bundle final (CSS generado en build). `styles.css` se elimina al migrar la última página.

### D5 — Rastro en miniatura (único cambio de backend, aditivo)

`GET /executions` añade por ejecución `pasos: [{ status, durationMs }]` en orden de cadena — mismo patrón que el `StepDto` del detalle pero **sin IO** (nada que enmascarar; menos bytes). Insert de la consulta: JOIN a `execution_steps` agregado por `execution_id` (una consulta extra por página como máximo, acotada al limit aplicado). La fila del listado renderiza segmentos flex proporcionales (`flex: duracionMs`, `min-width: 2px`, gap 1,5px) coloreados por estado, `skipped` atenuado; debajo, `N pasos · X ms` (o `en curso` con pasos running; `en cola` si no hay pasos). El detalle conserva el trace completo con IO enmascarado — eso no cambia.

### D6 — Estados de vista y responsive

Cada página parte de tres estados: **carga** (esqueleto neutro en tinta-tenue, sin spinners decorativos), **vacío** (una frase que nombra qué falta y la acción que lo resuelve — "Aún no hay puentes. Crea el primero." con botón si el rol permite), **error** (el Problem del servidor, qué pasó y qué reintentar). Responsive: en <900px la navegación colapsa a barra superior con selector, las tablas pasan a filas apiladas (etiqueta: valor) y los KPIs a rejilla 2×2; los botones de acción se mantienen alcanzables. Contraste AA verificado por token (test Vitest `tema.test.ts`, fuente de verdad `tema.css`): el tuning final oscureció `aviso` (#9A6B10 → #855D0E, para pasar sobre su propio fondo de insignia) y `tinta-tenue` (#8B9A93 → #64706B, AA sobre papel y superficie). Targets táctiles ≥ 40px; foco visible siempre.

### D7 — Instrucciones de escritura

Copys en español, voz de operador: verbos exactos ("Publicar versión" publica; el aviso que sigue dice "Versión publicada"), sin disculpas, errores que explican el qué y el cómo, vacíos que invitan. Sin puntos medios decorativos ni frases de marketing.

## Riesgos

- **Fuentes nuevas**: si `swap` tarda, el fallback es digno (system-ui); métrica objetivo: fuentes locales bellas, idioma español con acentos cubierto por los subsets latin/latin-ext.
- **Waterfall con datos ausentes**: duración `null` (pending/running) se renderiza como segmento en blanco de ancho mínimo con color de estado; ejecución sin pasos = sin segmentos, texto "en cola".
- **Tailwind v4 intro**: riesgo bajo — la API @theme es estable; se practica primero en el shell (F1-F2) antes de migrar páginas.

## Verificación

1:1 contra el target en 1440×900 (captura Playwright comparada con `target/d-el-rastro.png`: mismo encuadre, proporciones, tipografía, estados). Después: 1280 (desktop bajo), 390 (móvil), estados vacío/carga/error en al menos dos páginas, focus con teclado, reduced-motion. La comparación visual es humana (limitación D1); la automatizada es estructural + contraste.

### Desviaciones documentadas del target (resueltas en la implementación)

- La cabecera del target solo contiene rango + estados + puentes. Los filtros funcionales del listado que no estaban en el mockup (rango fino `desde` del MVP3 y auto-refresh del MVP5) viven en una barra propia dentro del panel de la tabla: cabecera idéntica al target, funcionalidad intacta.
- El target muestra «p95 · 1,8 s» como dato demo; `GET /stats` no expone p95 y este change no añade backend para métricas — el cuarto KPI muestra «media de exitosas».
- Densidad: filas de 47px vs 44px del mockup y ~11px más de aire sobre la cabecera; tolerancia aceptada (el rastro de dos líneas no comprime más sin perder legibilidad).
