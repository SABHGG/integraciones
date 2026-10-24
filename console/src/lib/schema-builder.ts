//! Constructor de JSON Schema del input del disparo (design D5): separa el
//! `inputSchema` vigente en campos simples editables (nombre/tipo/required) y
//! un "resto" que se conserva INTACTO (`pattern`, `additionalProperties`,
//! anidaciones raras…). Al montar, los campos editados se fusionan sobre el
//! resto: ninguna restricción fina se pierde por pasar por el formulario.

export type TipoCampo = "string" | "number" | "boolean" | "object" | "array";

export const TIPOS_CAMPO: TipoCampo[] = ["string", "number", "boolean", "object", "array"];

/** Un campo simple editable del schema. */
export interface CampoSchema {
  nombre: string;
  tipo: TipoCampo;
  requerido: boolean;
}

/** Schema desglosado: campos editables + resto conservado tal cual. */
export interface SchemaDesglosado {
  campos: CampoSchema[];
  resto: Record<string, unknown>;
}

function esObjeto(valor: unknown): valor is Record<string, unknown> {
  return typeof valor === "object" && valor !== null && !Array.isArray(valor);
}

/** Una definición de propiedad es editable-simple solo si su única clave es
 `type` con un tipo conocido: cualquier clave extra (pattern, enum…) la deja
 fuera del formulario para no destruirse al re-renderizar. */
function definicionEditable(def: unknown): def is { type: string } {
  return (
    esObjeto(def) &&
    Object.keys(def).length === 1 &&
    typeof def.type === "string" &&
    (TIPOS_CAMPO as string[]).includes(def.type)
  );
}

/** Desglosa un schema: los campos simples pasan al formulario; el resto
 (incluidas las propiedades no simples) queda congelado en `resto`. */
export function desglosarSchema(schema: unknown): SchemaDesglosado {
  if (!esObjeto(schema)) return { campos: [], resto: {} };

  const props = esObjeto(schema.properties) ? schema.properties : {};
  const required = Array.isArray(schema.required)
    ? schema.required.filter((r): r is string => typeof r === "string")
    : [];

  const campos: CampoSchema[] = [];
  const propsResto: Record<string, unknown> = {};
  const nombresEditables = new Set<string>();

  for (const [nombre, def] of Object.entries(props)) {
    if (definicionEditable(def)) {
      campos.push({ nombre, tipo: def.type as TipoCampo, requerido: required.includes(nombre) });
      nombresEditables.add(nombre);
    } else {
      propsResto[nombre] = def;
    }
  }

  const resto: Record<string, unknown> = { ...schema };
  delete resto.properties;
  delete resto.required;

  const requiredResto = required.filter((r) => !nombresEditables.has(r));
  if (requiredResto.length > 0) resto.required = requiredResto;
  if (Object.keys(propsResto).length > 0) resto.properties = propsResto;

  return { campos, resto };
}

/** Monta el JSON Schema final: campos del formulario fusionados sobre el resto
 conservado. `required` solo se emite si hay algo que requerir. */
export function montarSchema(campos: CampoSchema[], resto: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = { ...resto };

  const props: Record<string, unknown> = { ...(esObjeto(resto.properties) ? resto.properties : {}) };
  for (const campo of campos) {
    props[campo.nombre] = { type: campo.tipo };
  }
  if (Object.keys(props).length > 0) out.properties = props;
  else delete out.properties;

  const requiredResto = Array.isArray(resto.required) ? resto.required : [];
  const requeridos = [...new Set([...requiredResto, ...campos.filter((c) => c.requerido).map((c) => c.nombre)])];
  if (requeridos.length > 0) out.required = requeridos;
  else delete out.required;

  return out;
}
