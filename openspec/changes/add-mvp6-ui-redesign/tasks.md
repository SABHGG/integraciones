# Tasks

## 1. Fase 1 — Cimientos del sistema

- [x] 1.1 Copiar fuentes woff2 a `console/public/fonts/` con `@font-face` (swap, fallback system) y verificar carga sin red (build embebido)
- [x] 1.2 Añadir Tailwind v4 (`@tailwindcss/vite`) y definir `@theme` con los tokens de D2 (papel, tinta, estados, bordes, radios, tipografías)
- [x] 1.3 Extraer componentes base compartidos: botón (primario tinta / secundario borde / peligro), insignia de estado con LED, panel, campo con etiqueta y error, esqueleto de carga
- [x] 1.4 Verificar contraste AA de cada par token (script o cálculo en test de Vitest)

## 2. Fase 2 — Shell y acceso

- [x] 2.1 Rediseñar el shell: sidebar de tinta con marca, navegación, rol y salir; contenido a 1240px máx.
- [x] 2.2 Rediseñar login (email+contraseña / API key) con los mismos tokens y estados de error
- [x] 2.3 Colapso responsivo del shell <900px: barra superior con selector de sección; navegable a 390px

## 3. Fase 3 — Ejecuciones y el rastro (página target)

- [x] 3.1 Backend aditivo: `GET /executions` incluye `pasos` (status, durationMs, sin IO) por ejecución; test de API del campo nuevo; openapi.json actualizado
- [x] 3.2 Banda de métricas con divisores (total, éxito, fallos, duración media con p95) conectada a `GET /stats` con rango 7/30/90
- [x] 3.3 Serie diaria apilada correctas/fallidas con leyenda y eje de fechas
- [x] 3.4 Tabla de ejecuciones con rastro en miniatura por fila (segmentos proporcionales, total "N pasos · X ms" / "en curso" / "en cola"), filtros y auto-refresh
- [x] 3.5 Detalle de ejecución renovado: trace completo como cadena (nudo + rail), IO en mono, fallo señalizado; comparar 1:1 contra `target/`

## 4. Fase 4 — Puentes, editor y versiones

- [x] 4.1 Lista de puentes: tabla con slug/nombre/versión, export/duplicar/import por rol (editor crea, viewer exporta)
- [x] 4.2 Editor de puente: cadena de instancias con conectores de rastro, formularios `ui` y errores de validación sobre el campo
- [x] 4.3 Versiones: listado, diff lado a lado en mono semántico, rollback etiquetado

## 5. Fase 5 — Credenciales, bloques y miembros

- [x] 5.1 Credenciales: listado, creación, rotación e historial (jamás valores), estados de error de la bóveda
- [x] 5.2 Biblioteca de bloques: listado con filtros, detalle de versiones con schemas y `ui`, editor de código con errores en línea
- [x] 5.3 Miembros: listado y gestión por rol con confirmación de expulsión

## 6. Fase 6 — Estados, responsive y accesibilidad transversales

- [x] 6.1 Carga/vacío/error en las siete superficies, con copys de D7 (vacío que invita, error que explica)
- [x] 6.2 Responsive estructural: tablas apiladas <900px, KPIs 2×2, sin desbordes a 390px (checklist por página)
- [x] 6.3 Accesibilidad: foco visible en todos los controles, `prefers-reduced-motion` sin animaciones no esenciales, targets ≥40px
- [x] 6.4 Eliminar `styles.css` artesanal y clases muertas; suite Vitest de consola en verde

## 7. Fase 7 — Verificación visual y cierre

- [x] 7.1 Capturas Playwright (1440/1280/390) de todas las páginas y estados; comparación 1:1 de Ejecuciones contra `target/d-el-rastro.png` con registro de desviaciones y correcciones en lote
- [ ] 7.2 CI verde (api + Vitest consola) y README actualizado (guía de consola con el sistema visual y cómo extender tokens)
