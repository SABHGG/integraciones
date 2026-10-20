# Design

## Context

Ver `proposal.md — Why`. Estado actual y restricciones que condicionan el diseño:

- Consola **Preact 10 + Vite 6 + Tailwind v4** con tokens de tema en `console/src/tema.css` (`@theme`), build `tsc --noEmit && vite build` y tests vitest (`npm test`). Se sirve embebida por rust-embed desde `console/dist`: es **offline-first**, con fuentes autoalojadas y sin CDN.
- Shell y navegación en `console/src/main.tsx`: array `secciones` (Ejecuciones, Puentes, Miembros, Credenciales, Bloques), función `navegar(ruta)` y rutas por `popstate`.
- Sistema visual **El rastro** (`openspec/specs/web-console/spec.md`): el color solo significa estado, mono solo para valores de máquina, contraste AA vigilado por `console/src/tema.test.ts`, `prefers-reduced-motion` y foco visible ya soportados en `console/src/tema.css`.
- Roles `owner > editor > runner > viewer`; el espejo de presentación es `puede()` en `console/src/lib/roles.ts` (la matriz del servidor sigue siendo la única fuente de verdad).
- Regla dura del proyecto: la consola consume **solo** el plano admin existente. El tour no puede crear tabla, endpoint ni recurso.
- Lógica pura del proyecto vive en `console/src/lib/` y es la superficie natural para los tests unitarios.

## Goals / Non-Goals

**Goals:**

- Añadir el tour como una capa aditiva del shell, sin alterar rutas, páginas ni llamadas existentes.
- Concentrar la variabilidad (rol, versión del tour, selectores) en datos declarativos testeables en `console/src/lib/`, dejando la integración de UI delgada.
- Garantizar que el estado "ya lo vi" vive solo en el navegador y que un selector ausente nunca rompe el tour.
- Encajar la presentación del tour en El rastro sin introducir color fuera del sistema.

**Non-Goals:**

- Persistir el estado del tour en el servidor (ver `proposal.md — What Changes`, fuera de alcance).
- Diseñar tours por página independientes o un editor de tours.
- Gestionar variantes de idioma: el tour es en español, como el resto de la consola.
- Definir nombres concretos de variables CSS de driver.js: se decide el mapeo a tokens, no la implementación fina (ver Decisiones).

## Decisions

### D1 — driver.js como dependencia npm, no CDN

Se instala `driver.js` con `npm install driver.js` y se importa con `import { driver } from "driver.js"; import "driver.js/dist/driver.css";`. La vía CDN queda **prohibida**: rompe la filosofía offline-first (red sin salida a internet) y el test de ausencia de CDN ya existe en el sistema visual.

**Alternativas consideradas:** tour casero a base de popovers propios (más control, pero reimplementa foco, teclado, recorte y resaltado ya resueltos); CDN (descartado por la restricción anterior).

Coste aceptado: ~20 KB gzip en el bundle de la consola, evaluado como aceptable frente al valor de onboarding.

### D2 — Pasos declarados como datos en `console/src/lib/`

Los pasos del tour se declaran como un array de datos en `console/src/lib/` (p. ej. `lib/tour.ts`), con la forma `{ elemento: "#selector", titulo, descripcion, rolMinimo }`, y una función pura que, dado el rol, devuelve los pasos aplicables reutilizando el rango de rol existente (coherente con `puede()` de `lib/roles.ts`). `main.tsx` solo construye la instancia de driver.js a partir de esos datos.

**Por qué:** el filtrado por rol y la composición de pasos son la parte con reglas; como datos puros se testean con vitest sin DOM ni navegador, alineado con la convención del proyecto (`lib/` = lógica pura + tests).

**Alternativas consideradas:** pasos embebidos en el JSX del shell (difícil de testear y de mantener); pasos en JSON externo (añade carga asíncrona y un fallo de red potencial, sin ganancia).

### D3 — Clave y forma del `localStorage` con workspace y versión

El registro "ya lo vi" se guarda en `localStorage` bajo una clave estable y versionada del proyecto. La clave incorpora el **workspace** y el **valor** incorpora la **versión del tour** (una constante declarada junto a los pasos):

- Clave: identificador con prefijo de la consola + workspace (el workspace se obtiene de la sesión existente).
- Valor: número/etiqueta de versión del tour que el navegador ya vio.

Un helper puro `lib/` (`leerTourVisto` / `marcarTourVisto`) encapsula el acceso a `localStorage` y se testea con un stub de almacenamiento.

**Por qué:** indexar por workspace evita que un usuario con varios workspaces se lleve el tour visto de uno a otro; versionar el valor permite **invalidar** el tour cuando su contenido cambie sin tocar la clave. Acceder a `localStorage` puede lanzar (modo privado/cuota), así que el helper degrada a "no visto" sin romper la consola.

**Alternativas consideradas:** clave sin workspace (un workspace contamina a otro); solo clave versionada sin valor (obliga a cambiar la clave en cada iteración y no permite leer la versión previa).

### D4 — Botón "Ver tutorial" en el shell (`main.tsx`)

El disparador manual vive en el shell, junto a la navegación y al indicador de rol, **en la misma línea que el botón "Salir"**: en escritorio, junto al bloque de pie del sidebar de tinta; en móvil, en la barra superior. Se reutiliza el estilo del botón "Salir" (`Boton`/patrón de botón fantasma sobre tinta) para no introducir un componente ad hoc.

**Por qué:** el shell ya es el lugar del shell-wide chrome (rol y salir); el tour es una acción de sesión, no de página, y debe estar disponible desde cualquier ruta. Relanzar siempre reinicia desde el primer paso.

### D5 — Mapeo del popover a los tokens de El rastro

El popover y el resaltado se mapean a los tokens vigentes: papel/superficie de fondo, tinta para el texto, bordes del sistema y la tipografía grotesca de interfaz (mono reservada a valores de máquina, que aquí no aplica). El color del sistema solo significa estado, así que el tour **no** introduce verde/ladrillo/ámbar/azul decorativos.

El nombre concreto de cada variable CSS de driver.js se verificará durante la implementación leyendo `node_modules/driver.js`; en este diseño la decisión es **el mapeo semántico a tokens**, no los nombres.

**Por qué:** evita acoplar el plan a nombres de variables que pueden variar entre versiones de la biblioteca, y mantiene la regla de "sin estilos ad hoc por superficie" del sistema visual.

### D6 — Estrategia cuando un selector no existe

Antes de lanzar el tour, se resuelven los selectores declarados contra el DOM y **se filtran los pasos cuyo elemento no existe**; el tour arranca con los pasos resueltos. Si tras filtrar no queda ningún paso, el tour no se lanza (en vez de abrir un popover vacío) y no se marca como visto.

**Por qué:** el shell es responsive (una misma sección se ve distinta en móvil/escritorio) y los pasos apuntan a elementos que pueden no estar montados. Filtrar mantiene el requisito de degradación segura y evita fallos en cascada de driver.js.

**Alternativas consideradas:** confiar en el comportamiento de driver.js ante selectores ausentes (no garantizado entre versiones y puede detener el tour).

### D7 — Ciclo de vida y limpieza (`destroy`)

La instancia de driver.js se crea al lanzar y se destruye (`drv.destroy()`) al cerrar el tour, al navegar a otra ruta y al desmontar el shell (limpieza en el `useEffect` correspondiente). No hay estado global de tour: cada lanzamiento construye una instancia nueva.

**Por qué:** el shell cambia de ruta por `popstate` dentro de la misma SPA; sin destrucción quedarían popovers, resaltados y manejadores de teclado huérfanos. Esto materializa el requisito de limpieza correcta.

### D8 — Pruebas: unitarias puras + verificación manual del flujo

- **Unitarias (vitest, lógica pura):** filtrado de pasos por rol (viewer/runner/editor/owner, incluidos los casos negativos y los pasos comunes) y helper de `localStorage` (leer/marcar/versionar/workspace y degradación ante fallo de almacenamiento).
- **Verificación manual con `dev.sh`:** flujo end-to-end del tour por rol (primera visita auto, relanzado por botón, ausencia en segunda visita, movimiento reducido) — es interacción de navegador y no hay e2e de navegador en el proyecto.

**Por qué:** los tests deben cubrir reglas, no la librería; driver.js y el flujo visual se verifican en el navegador real durante el desarrollo.

## Risks / Trade-offs

- [El tour apunta a selectores del DOM y el shell responsive los altera] → D6: filtrado por existencia y no lanzar sin pasos; los selectores se declaran junto a los datos de los pasos, fácilmente ajustables.
- [driver.js añade ~20 KB gzip y una hoja de estilos externa] → Aceptado explícitamente (D1); la hoja se importa y se mapea a tokens (D5), sin CDN.
- [`localStorage` puede estar bloqueado (modo privado) o contener valores de otra versión] → D3: el helper degrada a "no visto" y el valor versionado se compara, nunca se asume.
- [Navegar dentro de la SPA con el tour abierto deja artefactos] → D7: `destroy` en cierre, navegación y desmontaje.
- [Deriva entre los pasos del tour y la UI por rol real] → El filtrado reutiliza el mismo rango de roles que `puede()`; cualquier cambio de matriz obliga a revisar `lib/tour.ts`, y las unitarias lo cubren.
- [El tour puede quedar obsoleto si cambian las páginas] → La versión del tour (D3) permite relanzarlo para todos al actualizarlo, sin migración de datos.

## Migration Plan

Aditivo, sin migración de datos ni de backend:

1. Publicar con el tour en su versión inicial: los navegadores sin registro lo verán una vez.
2. Rollback: revertir el commit de la consola; el valor en `localStorage` es inerte y no afecta al funcionamiento de la consola sin el tour.
3. Cambios futuros del contenido del tour: incrementar la constante de versión; los navegadores lo verán de nuevo automáticamente.

## Open Questions

Ninguna: los únicos detalles diferidos (nombres de variables CSS del popover de driver.js) están confinados a la implementación y no cambian las specs, el enfoque ni el desglose de tareas.
