//! Lógica pura de la biblioteca de bloques (MVP5, task 3.1): filtros del
//! listado y validación de creación en cliente. Sin DOM, testeable.

export interface ProblemaCampo {
  field: string;
  message: string;
}

export interface BloqueResumen {
  id: string;
  slug: string;
  name: string;
  blockType: string;
  category: string;
  description: string;
  latestVersion: number | null;
}

export interface FiltrosBloques {
  texto?: string;
  tipo?: string;
  categoria?: string;
}

/** Filtra el listado por texto libre (slug/nombre/descripción), tipo y categoría. */
export function filtrarBloques(
  bloques: BloqueResumen[],
  filtros: FiltrosBloques,
): BloqueResumen[] {
  const texto = (filtros.texto ?? "").trim().toLowerCase();
  return bloques.filter((bloque) => {
    if (filtros.tipo && bloque.blockType !== filtros.tipo) return false;
    if (filtros.categoria && bloque.category !== filtros.categoria) return false;
    if (texto !== "") {
      const pajar = `${bloque.slug} ${bloque.name} ${bloque.description}`.toLowerCase();
      if (!pajar.includes(texto)) return false;
    }
    return true;
  });
}

/** Categorías distintas del listado, ordenadas (para el selector). */
export function categorias(bloques: BloqueResumen[]): string[] {
  return [...new Set(bloques.map((b) => b.category))].sort();
}

const SLUG_VALIDO = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Creación de bloque custom_logic: slug y nombre obligatorios, slug kebab. */
export function validarCreacionBloque(form: {
  slug: string;
  name: string;
}): ProblemaCampo[] {
  const issues: ProblemaCampo[] = [];
  const slug = form.slug.trim();
  if (slug === "") {
    issues.push({ field: "slug", message: "el slug es obligatorio" });
  } else if (!SLUG_VALIDO.test(slug)) {
    issues.push({
      field: "slug",
      message: "usa minúsculas, números y guiones (p. ej. enrich-order)",
    });
  }
  if (form.name.trim() === "") {
    issues.push({ field: "name", message: "el nombre es obligatorio" });
  }
  return issues;
}

/**
 * Convierte el problema del servidor (detail + issues) en mensajes para
 * anotar sobre el editor de código.
 */
export function erroresDelPlano(detail: string, fields: { field: string; message: string }[]): string[] {
  const mensajes = fields.map((f) => `${f.field}: ${f.message}`);
  return [detail, ...mensajes].filter((m) => m !== "");
}
