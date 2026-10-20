import { describe, it, expect } from "vitest";
import { mapearErrores, lineaDeTranspilacion, type Issue } from "../src/lib/errors";

describe("mapearErrores", () => {
  it("reparte los issues del servidor entre globales y por instancia", () => {
    const issues: Issue[] = [
      { field: "trigger.method", message: "necesita method" },
      { field: "blocks.1.when", message: "ruta hacia atrás" },
      { field: "blocks.1.config.url", message: "url obligatoria" },
      { field: "blocks.0.code", message: "no transpila" },
      { field: "mask.0", message: "raíz desconocida" },
    ];
    const mapeado = mapearErrores(issues);
    expect(mapeado.globales).toHaveLength(2);
    expect(mapeado.porInstancia.get(0)).toEqual([
      { field: "blocks.0.code", message: "no transpila" },
    ]);
    expect(mapeado.porInstancia.get(1)).toHaveLength(2);
    expect(mapeado.porInstancia.get(2)).toBeUndefined();
  });

  it("sin issues devuelve vacío", () => {
    const mapeado = mapearErrores([]);
    expect(mapeado.globales).toHaveLength(0);
    expect(mapeado.porInstancia.size).toBe(0);
  });
});

describe("lineaDeTranspilacion", () => {
  it("extrae la línea del mensaje del transpilador", () => {
    expect(
      lineaDeTranspilacion("el código no compila (línea 3, columna 7): ';' expected"),
    ).toBe(3);
  });

  it("devuelve null si el mensaje no trae línea", () => {
    expect(lineaDeTranspilacion("el bloque necesita export default")).toBeNull();
  });
});
