import { describe, it, expect } from "vitest";
import { artefactoExport, validarImport, sufijarSlug } from "../src/lib/transferir";

const definicion = {
  trigger: { type: "http", method: "POST", path: "/x/uno" },
  blocks: [{ id: "b1", name: "eco" }],
};

describe("artefactoExport (task 6.1)", () => {
  it("contiene exactamente {name, slug, definition}, sin ids de workspace", () => {
    const texto = artefactoExport({
      name: "Puente uno",
      slug: "uno",
      definition: definicion,
    });
    const datos = JSON.parse(texto);
    expect(Object.keys(datos).sort()).toEqual(["definition", "name", "slug"]);
    expect(datos.name).toBe("Puente uno");
    expect(datos.definition).toEqual(definicion);
    expect(texto).not.toContain("workspace");
  });

  it("funciona igual con una definición congelada", () => {
    const texto = artefactoExport({
      name: "n",
      slug: "s",
      definition: { trigger: { type: "http" }, versionCongelada: true },
    });
    expect(JSON.parse(texto).definition.versionCongelada).toBe(true);
  });
});

describe("validarImport (task 6.2)", () => {
  it("acepta un artefacto válido", () => {
    const resultado = validarImport(
      JSON.stringify({ name: "X", slug: "x", definition: definicion }),
    );
    expect(resultado.ok).toBe(true);
    if (resultado.ok) {
      expect(resultado.datos.slug).toBe("x");
      expect(resultado.datos.definition).toEqual(definicion);
    }
  });

  it("rechaza JSON inválido con un error claro", () => {
    const resultado = validarImport("{esto no es json");
    expect(resultado.ok).toBe(false);
    if (!resultado.ok) expect(resultado.error).toContain("JSON");
  });

  it("rechaza JSON válido sin la forma {name, slug, definition}", () => {
    expect(validarImport('{"name": "solo nombre"}').ok).toBe(false);
    expect(validarImport('{"name": "x", "slug": "x", "definition": [1,2]}').ok).toBe(
      false,
    );
    expect(validarImport('{"name": "", "slug": "x", "definition": {}}').ok).toBe(
      false,
    );
  });
});

describe("sufijarSlug (task 6.3)", () => {
  it("sufija con -copia y vuelve a contar sobre copias previas", () => {
    expect(sufijarSlug("puente")).toBe("puente-copia");
    expect(sufijarSlug("puente-copia")).toBe("puente-copia2");
    expect(sufijarSlug("puente-copia2")).toBe("puente-copia3");
  });

  it("respeta kebab-case y longitud razonable", () => {
    const largo = "p".repeat(80);
    const resultado = sufijarSlug(largo);
    expect(resultado.length).toBeLessThanOrEqual(64);
    expect(resultado).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
  });
});
