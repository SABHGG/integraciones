//! Generador de formularios desde los metadatos `ui` de la versión del
//! bloque (contrato completo: `order[]`, `widget{}`, `help{}` con los 7 ids
//! de crates/api/src/ui.rs: text, url, number, textarea, password, select,
//! json, ref-template). Un bloque sin `ui` recibe un editor JSON genérico.

/** Metadatos `ui` de la versión del bloque (pueden venir vacíos). */
export interface UiMeta {
  order?: string[];
  widget?: Record<string, string>;
  help?: Record<string, string>;
}

/** Un campo del formulario generado. */
export interface CampoUi {
  nombre: string;
  widget: string;
  ayuda?: string;
  /** Opciones del `enum` del configSchema (solo widget `select`); vacío = cae a texto. */
  opciones?: unknown[];
}

/** Tipos de widget que la consola sabe pintar; el resto cae a texto. */
const WIDGETS_CONOCIDOS = new Set([
  "text", "url", "number", "textarea", "password", "select", "json", "ref-template",
]);

function esObjeto(valor: unknown): valor is Record<string, unknown> {
  return typeof valor === "object" && valor !== null && !Array.isArray(valor);
}

/** Opciones de un campo `select`: el `enum` que el configSchema declara para
 ese campo (JSON Schema estándar). Sin enum no hay opciones y el formulario
 cae a texto (design D1: el contrato ui no lleva opciones). */
function opcionesDeSelect(nombre: string, configSchema: unknown): unknown[] | undefined {
  if (!esObjeto(configSchema) || !esObjeto(configSchema.properties)) return undefined;
  const def = configSchema.properties[nombre];
  if (!esObjeto(def) || !Array.isArray(def.enum)) return undefined;
  return def.enum;
}

/**
 * Campos del formulario en el orden declarado, con su widget, ayuda y opciones.
 * Devuelve `null` si el bloque no tiene metadatos `ui` útiles → la pantalla
 * muestra el editor JSON genérico.
 */
export function camposDeUi(ui: UiMeta | null | undefined, configSchema?: unknown): CampoUi[] | null {
  if (!ui || !Array.isArray(ui.order) || ui.order.length === 0) {
    return null;
  }
  return ui.order.map((nombre) => {
    const widget = normalizarWidget(ui.widget?.[nombre]);
    return {
      nombre,
      widget,
      ayuda: ui.help?.[nombre],
      ...(widget === "select" ? { opciones: opcionesDeSelect(nombre, configSchema) } : {}),
    };
  });
}

function normalizarWidget(widget: string | undefined): string {
  const id = widget ?? "text";
  return WIDGETS_CONOCIDOS.has(id) ? id : "text";
}

/**
 * Valor inicial de un campo según su widget (para no enviar strings donde el
 * schema espera número).
 */
export function valorDeCampo(widget: string, crudo: string): unknown {
  if (widget === "number") {
    const numero = Number(crudo);
    return crudo.trim() === "" || Number.isNaN(numero) ? crudo : numero;
  }
  return crudo;
}
