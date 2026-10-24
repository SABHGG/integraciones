//! Tests de la autoría de metadatos ui (G6): documento conforme al contrato
//! cerrado de crates/api/src/ui.rs y prellenado desde la versión publicada.
import { describe, expect, it } from "vitest";
import { AUTORIA_VACIA, construirUi, uiDeAutoria } from "./ui-meta";

describe("construirUi", () => {
  it("sin campos declarados devuelve null (no se envía ui)", () => {
    expect(construirUi(AUTORIA_VACIA)).toBeNull();
    expect(construirUi({ title: "X", campos: ["  ", ""], widgets: {}, helps: {} })).toBeNull();
  });

  it("emite order limpio (sin vacíos ni duplicados) y title solo si lo hay", () => {
    const ui = construirUi({
      title: "  Configurar personaje  ",
      campos: ["url", "", "url", "timeoutMs"],
      widgets: {},
      helps: {},
    });
    expect(ui).toEqual({ title: "Configurar personaje", order: ["url", "timeoutMs"] });
  });

  it("widget: solo ids válidos, solo de order, text omitido (es el default)", () => {
    const ui = construirUi({
      title: "",
      campos: ["url", "method", "extra"],
      widgets: { url: "url", method: "select", extra: "toggle-raro", fantasma: "number" },
      helps: {},
    });
    expect(ui).toEqual({ order: ["url", "method", "extra"], widget: { url: "url", method: "select" } });
  });

  it("help: solo textos no vacíos de campos de order", () => {
    const ui = construirUi({
      title: "",
      campos: ["url", "timeoutMs"],
      widgets: {},
      helps: { url: "  Endpoint completo  ", timeoutMs: "   ", otro: "x" },
    });
    expect(ui).toEqual({ order: ["url", "timeoutMs"], help: { url: "Endpoint completo" } });
  });
});

describe("uiDeAutoria", () => {
  it("ida y vuelta: construir → parsear reproduce la autoría declarada", () => {
    const autoria = {
      title: "Llamada HTTP",
      campos: ["url", "method"],
      widgets: { url: "url", method: "select" },
      helps: { url: "Endpoint" },
    };
    const ui = construirUi(autoria);
    // El prellenado da entrada a TODO campo de order (help vacío = sin help).
    expect(uiDeAutoria(ui)).toEqual({ ...autoria, helps: { url: "Endpoint", method: "" } });
  });

  it("ui vacío, basura o sin order: autoría vacía", () => {
    expect(uiDeAutoria(null)).toEqual(AUTORIA_VACIA);
    expect(uiDeAutoria({})).toEqual(AUTORIA_VACIA);
    expect(uiDeAutoria("x")).toEqual(AUTORIA_VACIA);
    expect(uiDeAutoria({ order: [] })).toEqual(AUTORIA_VACIA);
  });

  it("un ui parcial (order con widget desconocido) precarga con defaults", () => {
    expect(uiDeAutoria({ order: ["a", "b"], widget: { a: "json" } })).toEqual({
      title: "",
      campos: ["a", "b"],
      widgets: { a: "json", b: "text" },
      helps: { a: "", b: "" },
    });
  });
});
