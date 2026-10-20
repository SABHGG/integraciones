import { describe, it, expect } from "vitest";
import { camposDeUi, valorDeCampo, type UiMeta } from "../src/lib/ui-forms";

describe("camposDeUi", () => {
  it("genera los campos en el orden declarado con widget y ayuda", () => {
    const ui: UiMeta = {
      order: ["url", "timeout", "headers"],
      widget: { url: "url", timeout: "number", headers: "textarea" },
      help: { url: "la https de destino" },
    };
    const campos = camposDeUi(ui)!;
    expect(campos.map((c) => c.nombre)).toEqual(["url", "timeout", "headers"]);
    expect(campos[0]).toMatchObject({ widget: "url", ayuda: "la https de destino" });
    expect(campos[1].widget).toBe("number");
  });

  it("un widget desconocido cae a texto", () => {
    const campos = camposDeUi({ order: ["x"], widget: { x: "galaxia" } })!;
    expect(campos[0].widget).toBe("text");
  });

  it("sin ui (o sin order) devuelve null → editor JSON genérico", () => {
    expect(camposDeUi(null)).toBeNull();
    expect(camposDeUi({})).toBeNull();
    expect(camposDeUi({ order: [] })).toBeNull();
  });

  it("sin widget declarado el campo es texto", () => {
    const campos = camposDeUi({ order: ["a", "b"] })!;
    expect(campos.map((c) => c.widget)).toEqual(["text", "text"]);
  });
});

describe("valorDeCampo", () => {
  it("number convierte a número cuando puede", () => {
    expect(valorDeCampo("number", "42")).toBe(42);
    expect(valorDeCampo("number", "")).toBe("");
    expect(valorDeCampo("number", "no-es")).toBe("no-es");
  });

  it("texto/textarea/url/password devuelven el string tal cual", () => {
    expect(valorDeCampo("text", "hola")).toBe("hola");
    expect(valorDeCampo("url", "https://x.y")).toBe("https://x.y");
  });
});
