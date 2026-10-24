//! Autoría de los metadatos `ui` de una versión de bloque (design D6): el
//! formulario de publicación construye el documento del contrato cerrado de
//! crates/api/src/ui.rs (title | order | widget | help, ids kebab-case). Solo
//! se envía `ui` cuando el autor declaró algo: sin metadatos la publicación
//! va byte a byte igual que antes (el servidor guarda `ui = {}`).

/** Estado de autoría en el formulario de publicación. */
export interface AutoriaUi {
  title: string;
  /** Nombres de campo en el orden de pintado (order[]). */
  campos: string[];
  /** Campo → id de widget elegido ("text" = el default, no se emite). */
  widgets: Record<string, string>;
  /** Campo → texto de ayuda (vacío = no se emite). */
  helps: Record<string, string>;
}

/** Los 7 ids de widget del contrato (ui.rs). */
export const IDS_WIDGET_UI = [
  "text", "url", "number", "textarea", "password", "select", "json", "ref-template",
] as const;

export const AUTORIA_VACIA: AutoriaUi = { title: "", campos: [], widgets: {}, helps: {} };

const WIDGETS_VALIDOS = new Set<string>(IDS_WIDGET_UI);

/** Construye el documento `ui` a enviar en la publicación; `null` si no hay
 campos declarados (no se envía `ui`). Limpia nombres vacíos y duplicados
 (el contrato exige order único y no vacío) y emite solo widget/help con
 contenido, siempre referidos a campos de order. */
export function construirUi(a: AutoriaUi): Record<string, unknown> | null {
  const vistas = new Set<string>();
  const order: string[] = [];
  for (const crudo of a.campos) {
    const nombre = crudo.trim();
    if (nombre === "" || vistas.has(nombre)) continue;
    vistas.add(nombre);
    order.push(nombre);
  }
  if (order.length === 0) return null;

  const ui: Record<string, unknown> = {};
  const title = a.title.trim();
  if (title !== "") ui.title = title;
  ui.order = order;

  const widget: Record<string, string> = {};
  for (const campo of order) {
    const id = a.widgets[campo];
    if (id && WIDGETS_VALIDOS.has(id) && id !== "text") widget[campo] = id;
  }
  if (Object.keys(widget).length > 0) ui.widget = widget;

  const help: Record<string, string> = {};
  for (const campo of order) {
    const texto = (a.helps[campo] ?? "").trim();
    if (texto !== "") help[campo] = texto;
  }
  if (Object.keys(help).length > 0) ui.help = help;

  return ui;
}

/** Carga la autoría desde el `ui` de la última versión publicada (prellenado
 del formulario): lo ya publicado se conserva al republicar. */
export function uiDeAutoria(ui: unknown): AutoriaUi {
  if (typeof ui !== "object" || ui === null || Array.isArray(ui)) return structuredClone(AUTORIA_VACIA);
  const doc = ui as Record<string, unknown>;
  const order = Array.isArray(doc.order)
    ? doc.order.filter((x): x is string => typeof x === "string")
    : [];
  const mapa = (valor: unknown): Record<string, unknown> =>
    typeof valor === "object" && valor !== null && !Array.isArray(valor)
      ? (valor as Record<string, unknown>)
      : {};
  const widget = mapa(doc.widget);
  const help = mapa(doc.help);
  const textoDe = (registro: Record<string, unknown>, campo: string, porDefecto: string) =>
    typeof registro[campo] === "string" ? (registro[campo] as string) : porDefecto;
  return {
    title: textoDe(doc, "title", ""),
    campos: order,
    widgets: Object.fromEntries(order.map((c) => [c, textoDe(widget, c, "text")])),
    helps: Object.fromEntries(order.map((c) => [c, textoDe(help, c, "")])),
  };
}
