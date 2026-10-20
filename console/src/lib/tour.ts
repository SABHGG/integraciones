//! Tour guiado de onboarding (change `add-console-guided-tour`): los pasos se
//! declaran aquí como datos puros y `main.tsx` solo los convierte en pasos de
//! driver.js. Concentrar aquí la variabilidad (rol, versión, selectores y
//! almacenamiento) permite testear las reglas con vitest sin DOM.
//!
//! Reglas del sistema El rastro que este módulo respeta: el color no entra en
//! los datos (los estilos viven en tema.css), la mono no aparece porque ningún
//! paso describe un valor de máquina, y el tour no navega entre páginas: los
//! pasos de una página se filtran cuando esa página no está montada (D6).
import { puede, type Accion, type Rol } from "./roles";

/// Versión del contenido del tour. Al cambiarlo, los navegadores que lo tenían
/// visto lo vuelven a recibir (D3): el valor guardado se compara, no se asume.
export const VERSION_TOUR = 1;

/// Prefijo de la clave en `localStorage`; el workspace viaja en la clave.
export const CLAVE_TOUR = "integraciones.tour";

/// Un paso del tour: qué elemento resalta, qué cuenta y desde qué rol aplica.
export interface PasoDelTour {
  /** Selector CSS del elemento a resaltar (se valida al lanzar, D6). */
  elemento: string;
  titulo: string;
  descripcion: string;
  /** Rol mínimo que ve el paso (mismo rango que `puede()` de lib/roles.ts). */
  rolMinimo: Rol;
}

/// Pasos declarativos del tour. El orden es el del recorrido: shell, dashboard
/// y trace (comunes), y después las superficies por rol. Los selectores de
/// página se anclan a `[data-pantalla="…"]`, atributo que `main.tsx` pone en el
/// contenedor de la página vigente, de modo que un paso de otra página no
/// resalta el elemento homónimo de la actual.
export const PASOS_TOUR: readonly PasoDelTour[] = [
  {
    elemento: 'aside nav[aria-label="Secciones"]',
    titulo: "Las cinco secciones",
    descripcion:
      "Toda la consola vive aquí: Ejecuciones, Puentes, Miembros, Credenciales y Bloques. Cada página hace una sola cosa y ninguna acción existe fuera de tu rol.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'header select[aria-label="Sección"]',
    titulo: "La misma navegación en móvil",
    descripcion:
      "En pantallas estrechas el sidebar se convierte en este selector: se eligen las mismas cinco secciones.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'aside nav a[href="/console/executions"]',
    titulo: "Ejecuciones: el dashboard",
    descripcion:
      "El rastro de invocación de tus puentes: salud del workspace, listado filtrable y el detalle paso a paso de cada ejecución.",
    rolMinimo: "viewer",
  },
  {
    elemento: '[data-pantalla="ejecuciones"] [role="group"][aria-label="Rango de fechas"]',
    titulo: "Métricas por rango",
    descripcion:
      "La banda de arriba resume el rango elegido: total, estados, duración media y tendencia diaria. Cambiar 7/30/90 días recalcula todo.",
    rolMinimo: "viewer",
  },
  {
    elemento: '[data-pantalla="ejecuciones"] table',
    titulo: "El rastro en miniatura",
    descripcion:
      "Cada fila dibuja un segmento por paso, proporcional a su duración y coloreado por estado: de un vistazo ves qué bloque es lento o falló. Un clic abre el detalle.",
    rolMinimo: "viewer",
  },
  {
    elemento: '[data-pantalla="ejecuciones"] select[aria-label="Filtrar por estado"]',
    titulo: "Filtros del listado",
    descripcion:
      "Filtra por puente, estado o fecha. El auto-refresh es opcional (cada 5 s) y se pausa solo cuando la pestaña está oculta.",
    rolMinimo: "viewer",
  },
  {
    elemento: '[data-pantalla="ejecucion"] ol',
    titulo: "El trace paso a paso",
    descripcion:
      "El detalle muestra cada paso con su estado, su duración y la entrada/salida ya enmascarada. El punto exacto del fallo se señala donde ocurre.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'aside nav a[href="/console/bridges"]',
    titulo: "Puentes",
    descripcion:
      "La definición de cada puente y su panel de versiones: puedes leer la cadena de bloques, comparar versiones y ver en qué quedó cada publicación.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'aside nav a[href="/console/blocks"]',
    titulo: "Bloques",
    descripcion:
      "La biblioteca de piezas reutilizables por tus puentes, con sus versiones, schemas y metadatos de formulario.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'aside nav a[href="/console/credentials"]',
    titulo: "Credenciales",
    descripcion:
      "La bóveda write-only: ves nombres, versiones vigentes y fechas — nunca los valores. Cada ejecución audita qué versión usó.",
    rolMinimo: "viewer",
  },
  {
    elemento: 'aside nav a[href="/console/members"]',
    titulo: "Miembros",
    descripcion:
      "Quién pertenece al workspace y con qué rol. Como owner puedes añadir personas por email, cambiar su rol y expulsarlas: el cambio surte efecto al instante.",
    rolMinimo: "owner",
  },
  {
    elemento: '[data-pantalla="puente"] > div:first-child > div:first-child',
    titulo: "Editar, validar y publicar",
    descripcion:
      "Con rol editor o superior, el editor del puente guarda el borrador, valida y publica una versión congelada; el dry-run ensaya con datos de prueba sin generar filas.",
    rolMinimo: "editor",
  },
  {
    elemento: '[data-pantalla="credenciales"] form',
    titulo: "Crear y rotar credenciales",
    descripcion:
      "Con rol editor o superior puedes crear un secreto y rotarlo: el valor se cifra en reposo y no se vuelve a mostrar jamás.",
    rolMinimo: "editor",
  },
  {
    elemento: "aside > div:last-child",
    titulo: "Tu rol, y este tour",
    descripcion:
      "Aquí ves tu rol y cierras sesión. «Ver tutorial» relanza este recorrido desde el principio siempre que quieras.",
    rolMinimo: "viewer",
  },
];

/// Acción de `lib/roles.ts` equivalente a cada rol mínimo: así el filtrado
/// reutiliza el rango `owner > editor > runner > viewer` sin duplicarlo.
const ACCION_POR_ROL: Record<Rol, Accion> = {
  viewer: "leer",
  runner: "ejecutar",
  editor: "administrar-recursos",
  owner: "gestionar-miembros",
};

/// Filtra los pasos aplicables a un rol reutilizando el rango de `lib/roles.ts`
/// y, opcionalmente, los selectores que existan en el DOM en ese momento (D6).
/// `existe` se inyecta para poder testear el filtrado sin DOM; omitirlo deja
/// solo el filtro por rol.
export function pasosDisponibles(
  rol: Rol | null | undefined,
  existe: (selector: string) => boolean = () => true,
): PasoDelTour[] {
  return PASOS_TOUR.filter(
    (paso) => puede(rol, ACCION_POR_ROL[paso.rolMinimo]) && existe(paso.elemento),
  );
}

/// Almacén mínimo que necesita el helper (lo cumple `localStorage`).
export interface Almacen {
  getItem(clave: string): string | null;
  setItem(clave: string, valor: string): void;
}

/// `localStorage` del navegador, o `null` si no existe o el acceso lanza
/// (modo privado): el tour degrada a "no visto" sin romper la consola (D3).
function almacenDelNavegador(): Almacen | null {
  try {
    if (typeof localStorage === "undefined") return null;
    return localStorage;
  } catch {
    return null;
  }
}

/// Clave por workspace: un workspace no contamina a otro (D3).
export function claveTour(workspace: string | null | undefined): string {
  const id =
    typeof workspace === "string" && workspace.trim() !== "" ? workspace.trim() : "sin-workspace";
  return `${CLAVE_TOUR}.${id}`;
}

/// ¿Este navegador ya vio la versión vigente del tour en este workspace?
export function leerTourVisto(
  workspace: string | null | undefined,
  almacen: Almacen | null = almacenDelNavegador(),
): boolean {
  if (!almacen) return false;
  try {
    return almacen.getItem(claveTour(workspace)) === String(VERSION_TOUR);
  } catch {
    return false;
  }
}

/// Registra que este navegador vio la versión vigente del tour en este
/// workspace. Un fallo de almacenamiento (cuota, modo privado) no propaga.
export function marcarTourVisto(
  workspace: string | null | undefined,
  almacen: Almacen | null = almacenDelNavegador(),
): void {
  if (!almacen) return;
  try {
    almacen.setItem(claveTour(workspace), String(VERSION_TOUR));
  } catch {
    // Sin persistencia: el tour volverá a lanzarse la próxima visita.
  }
}

/// ¿El sistema pide movimiento reducido? El tour evita entonces animaciones y
/// desplazamiento suave (tema.css refuerza la regla por CSS).
export function movimientoReducido(): boolean {
  try {
    return (
      typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches
    );
  } catch {
    return false;
  }
}
