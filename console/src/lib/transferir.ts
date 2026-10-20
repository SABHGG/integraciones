//! Export/import/duplicar puentes (MVP5, tasks 6.1-6.3, design D5): el
//! artefacto es `{name, slug, definition}` SIN ids de workspace; el
//! import valida contra el plano (una sola fuente de verdad). Lógica
//! pura testeable; la descarga (Blob) y las llamadas viven en la UI.

export interface ArtefactoPuente {
  name: string;
  slug: string;
  definition: unknown;
}

export type ResultadoImport =
  | { ok: true; datos: ArtefactoPuente }
  | { ok: false; error: string };

/**
 * Serializa el artefacto de export: definición (borrador o versión
 * congelada) más nombre y slug — jamás ids de workspace ni de versión.
 */
export function artefactoExport(artefacto: ArtefactoPuente): string {
  return JSON.stringify(
    { name: artefacto.name, slug: artefacto.slug, definition: artefacto.definition },
    null,
    2,
  );
}

/**
 * Valida el texto pegado/cargado antes de tocar el plano: JSON válido con
 * la forma `{name, slug, definition}` (definition objeto, no array).
 */
export function validarImport(texto: string): ResultadoImport {
  let datos: unknown;
  try {
    datos = JSON.parse(texto);
  } catch {
    return { ok: false, error: "el texto no es JSON válido" };
  }
  const artefacto = datos as Partial<ArtefactoPuente> | null;
  const definition = artefacto?.definition;
  const definitionOk =
    definition !== null &&
    typeof definition === "object" &&
    !Array.isArray(definition);
  if (
    artefacto === null ||
    typeof artefacto !== "object" ||
    typeof artefacto.name !== "string" ||
    artefacto.name.trim() === "" ||
    typeof artefacto.slug !== "string" ||
    artefacto.slug.trim() === "" ||
    !definitionOk
  ) {
    return { ok: false, error: "el artefacto debe ser {name, slug, definition}" };
  }
  return {
    ok: true,
    datos: {
      name: artefacto.name as string,
      slug: artefacto.slug as string,
      definition,
    },
  };
}

const SLUG_VALIDO = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Slug sufijado para duplicar: `puente` → `puente-copia` (y `-copia2`,
 * `-copia3`… si ya lo era). Respeta el máximo razonable de 64 caracteres.
 */
export function sufijarSlug(slug: string): string {
  const previa = slug.match(/^(.*)-copia(\d*)$/);
  if (previa !== null) {
    const base = previa[1].slice(0, 56);
    const numero = previa[2] === "" ? 1 : parseInt(previa[2], 10);
    const candidato = `${base}-copia${numero + 1}`;
    if (SLUG_VALIDO.test(candidato)) return candidato;
  }
  const candidato = `${slug.replace(/-copia\d*$/, "").slice(0, 56)}-copia`;
  if (SLUG_VALIDO.test(candidato)) return candidato;
  return `copia-${Math.abs(hashTextual(slug)) % 100000}`;
}

function hashTextual(texto: string): number {
  let hash = 0;
  for (let i = 0; i < texto.length; i += 1) {
    hash = (hash * 31 + texto.charCodeAt(i)) | 0;
  }
  return hash;
}
