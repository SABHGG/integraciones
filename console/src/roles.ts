//! Espejo de la matriz de roles del servidor SOLO para la UI: qué acciones
//! se ocultan/deshabilitan según el rol del `whoami`. La única fuente de
//! verdad sigue siendo el servidor (los 403 se muestran donde ocurran);
//! esto es solo presentación.

export type Rol = "owner" | "editor" | "runner" | "viewer";

/** Acciones de la consola y el rol mínimo que las ve. */
export const ACCIONES = {
  leer: "viewer",
  ejecutar: "runner",
  "administrar-recursos": "editor",
  "gestionar-miembros": "owner",
} as const;

export type Accion = keyof typeof ACCIONES;

const RANGO: Record<Rol, number> = { viewer: 0, runner: 1, editor: 2, owner: 3 };

/** ¿La UI debe mostrar/habilitar esta acción para este rol? */
export function puede(rol: Rol | null | undefined, accion: Accion): boolean {
  if (!rol) return false;
  const requerido = ACCIONES[accion] as Rol;
  return RANGO[rol] >= RANGO[requerido];
}

/** Etiqueta legible del rol para la cabecera. */
export function etiquetaRol(rol: Rol | null | undefined): string {
  switch (rol) {
    case "owner":
      return "owner";
    case "editor":
      return "editor";
    case "runner":
      return "runner";
    case "viewer":
      return "viewer";
    default:
      return "sin rol";
  }
}
