//! Validación de los formularios de credenciales (MVP5, task 2.1).
//! Lógica pura, sin DOM: testeable con Vitest. Los errores del servidor
//! (422 del validador y 409) se muestran en línea en la página; esto solo
//! evita peticiones evidentemente inválidas.

export interface ProblemaCampo {
  field: string;
  message: string;
}

export interface FormularioCreacion {
  name: string;
  secret: string;
  description?: string;
}

/** Creación: nombre y secreto obligatorios. */
export function validarCreacion(form: FormularioCreacion): ProblemaCampo[] {
  const issues: ProblemaCampo[] = [];
  if (form.name.trim() === "") {
    issues.push({ field: "name", message: "el nombre es obligatorio" });
  }
  if (form.secret === "") {
    issues.push({ field: "secret", message: "el secreto es obligatorio" });
  }
  return issues;
}

/** Rotación: el valor nuevo es obligatorio (write-only: nunca se pre-llena). */
export function validarRotacion(secret: string): ProblemaCampo[] {
  if (secret === "") {
    return [{ field: "secret", message: "introduce el valor nuevo para rotar" }];
  }
  return [];
}

/**
 * Eliminación con confirmación escrita: hay que teclear el nombre de la
 * credencial (una operación irreversible no se confirma con un clic).
 */
export function confirmarEliminacion(nombre: string, escrito: string): boolean {
  return escrito.trim() === nombre.trim() && nombre.trim() !== "";
}

/** Indica qué versión del historial es la vigente. */
export function esVigente(versionId: string, currentVersionId: string | null): boolean {
  return currentVersionId !== null && versionId === currentVersionId;
}
