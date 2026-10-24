//! Tests de los metadatos ui ampliados: los 7 widgets del contrato y las
//! opciones de `select` desde el enum del configSchema (design D1).
import { describe, expect, it } from "vitest";
import { camposDeUi, valorDeCampo } from "./ui-forms";

describe("camposDeUi", () => {
  it("reconoce los 7 widgets del contrato; ids desconocidos caen a text", () => {
    const campos = camposDeUi({
      order: ["a", "b", "c", "d", "e", "f", "g", "h"],
      widget: {
        a: "text", b: "url", c: "number", d: "textarea",
        e: "password", f: "select", g: "json", h: "ref-template",
      },
    });
    expect(campos?.map((c) => c.widget)).toEqual([
      "text", "url", "number", "textarea", "password", "select", "json", "ref-template",
    ]);
    expect(camposDeUi({ order: ["x"], widget: { x: "toggle-raro" } })?.[0].widget).toBe("text");
  });

  it("select toma las opciones del enum del configSchema del campo", () => {
    const campos = camposDeUi(
      { order: ["method"], widget: { method: "select" } },
      { properties: { method: { type: "string", enum: ["GET", "POST"] } } },
    );
    expect(campos?.[0].opciones).toEqual(["GET", "POST"]);
  });

  it("select sin configSchema o sin enum no trae opciones (cae a texto en el formulario)", () => {
    expect(camposDeUi({ order: ["m"], widget: { m: "select" } })?.[0].opciones).toBeUndefined();
    expect(
      camposDeUi({ order: ["m"], widget: { m: "select" } }, { properties: { m: { type: "string" } } })?.[0].opciones,
    ).toBeUndefined();
  });

  it("sin ui útil devuelve null (editor JSON genérico)", () => {
    expect(camposDeUi(null)).toBeNull();
    expect(camposDeUi({ order: [] })).toBeNull();
    expect(camposDeUi(undefined)).toBeNull();
  });

  it("ayuda viaja por campo", () => {
    const campos = camposDeUi({ order: ["url"], help: { url: "Endpoint completo" } });
    expect(campos?.[0].ayuda).toBe("Endpoint completo");
  });
});

describe("valorDeCampo", () => {
  it("number convierte y conserva crudo si no es número", () => {
    expect(valorDeCampo("number", "42")).toBe(42);
    expect(valorDeCampo("number", "abc")).toBe("abc");
    expect(valorDeCampo("number", "  ")).toBe("  ");
  });
  it("los demás widgets quedan como string", () => {
    expect(valorDeCampo("text", "hola")).toBe("hola");
    expect(valorDeCampo("url", "https://x")).toBe("https://x");
  });
});
