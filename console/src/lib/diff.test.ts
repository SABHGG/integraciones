//! Tests del filtrado de líneas por secciones (add-publish-diff T1): solo
//! secciones editables del borrador y `blocks.*.code` (resolución) fuera.
import { describe, expect, it } from "vitest";
import { diffEstructural, lineasDeSecciones, type LineaDiff } from "./diff";

const SECCIONES = ["trigger", "inputSchema", "blocks", "mask", "emit", "webhook"] as const;

const linea = (ruta: string): LineaDiff => ({ ruta, izq: "v1", der: "v2" });

describe("lineasDeSecciones", () => {
  it("conserva las rutas de las secciones editables", () => {
    const rutas = ["trigger.method", "inputSchema.required", "blocks.1.config.url", "mask.0", "emit.topic", "webhook.url"];
    expect(lineasDeSecciones(rutas.map(linea), SECCIONES).map((l) => l.ruta)).toEqual(rutas);
  });

  it("descarta rutas fuera de las secciones y la raíz completa", () => {
    const filtradas = lineasDeSecciones([linea("workspaceId"), linea("$"), linea("trigger.method")], SECCIONES);
    expect(filtradas.map((l) => l.ruta)).toEqual(["trigger.method"]);
  });

  it("descarta blocks.*.code (resolución) pero no blocks.*.codeTranspilado-tipo-rutas hijas distintas", () => {
    const filtradas = lineasDeSecciones(
      [linea("blocks.0.code"), linea("blocks.0.code.0"), linea("blocks.0.config.url")],
      SECCIONES,
    );
    expect(filtradas.map((l) => l.ruta)).toEqual(["blocks.0.config.url"]);
  });

  it("bloques.10.code también cae (índices multidígitos)", () => {
    expect(lineasDeSecciones([linea("blocks.10.code"), linea("blocks.10.name")], SECCIONES).map((l) => l.ruta)).toEqual([
      "blocks.10.name",
    ]);
  });

  it("sin líneas: lista vacía", () => {
    expect(lineasDeSecciones([], SECCIONES)).toEqual([]);
  });
});

describe("integración con diffEstructural", () => {
  it("borrador vs definición congelada: ruido de resolución filtrado", () => {
    const borrador = {
      trigger: { type: "http", method: "POST", path: "/x/demo" },
      inputSchema: { type: "object" },
      blocks: [{ id: "b1", name: "enrich", blockId: "x", blockVersionId: "v1", input: {}, config: { url: "/nueva" } }],
    };
    const definition = {
      trigger: { type: "http", method: "POST", path: "/x/demo" },
      inputSchema: { type: "object" },
      blocks: [
        {
          id: "b1",
          name: "enrich",
          blockId: "x",
          blockVersionId: "v1",
          input: {},
          config: { url: "/vieja" },
          code: "console.log('resuelto')",
        },
      ],
    };
    const lineas = lineasDeSecciones(diffEstructural(borrador, definition), SECCIONES);
    expect(lineas.map((l) => l.ruta)).toEqual(["blocks.0.config.url"]);
    expect(lineas[0].izq).toBe("/nueva");
    expect(lineas[0].der).toBe("/vieja");
  });
});
