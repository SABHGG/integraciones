//! Generador de formularios desde los metadatos `ui` de la versión del
//! bloque (contrato MVP1: `order[]`, `widget{}`, `help{}`). Un bloque sin
//! `ui` recibe un editor JSON genérico.

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
}

/** Tipos de widget que la consola sabe pintar; el resto cae a texto. */
const WIDGETS_CONOCIDOS = new Set(["text", "url", "number", "textarea", "password"]);

/**
 * Campos del formulario en el orden declarado, con su widget y ayuda.
 * Devuelve `null` si el bloque no tiene metadatos `ui` útiles → la pantalla
 * muestra el editor JSON genérico.
 */
export function camposDeUi(ui: UiMeta | null | undefined): CampoUi[] | null {
  if (!ui || !Array.isArray(ui.order) || ui.order.length === 0) {
    return null;
  }
  return ui.order.map((nombre) => ({
    nombre,
    widget: normalizarWidget(ui.widget?.[nombre]),
    ayuda: ui.help?.[nombre],
  }));
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
