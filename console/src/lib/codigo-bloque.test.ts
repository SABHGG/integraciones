//! Decisión pura del visor de código del detalle de bloque (change
//! add-block-source-code, design D5). Sin DOM: `environment: "node"` en vitest.
import { describe, expect, it } from "vitest";
import { codigoAMostrar, type VersionConCodigo } from "./codigo-bloque";

function version(campos: Partial<VersionConCodigo>): VersionConCodigo {
  return { code: null, sourceCode: null, ...campos };
}

describe("codigoAMostrar", () => {
  it("con fuente original muestra el TypeScript y sin etiqueta", () => {
    const ts = "export default (input: any) => input;\n";
    const js = "export default (input)=>input;\n";
    expect(codigoAMostrar(version({ sourceCode: ts, code: js }))).toEqual({
      codigo: ts,
      etiqueta: null,
    });
  });

  it("sin fuente y con JS lo etiqueta como JS transpilado de versión anterior", () => {
    const js = "export default (input)=>input;\n";
    expect(codigoAMostrar(version({ code: js }))).toEqual({
      codigo: js,
      etiqueta: "JS transpilado — versión anterior al change",
    });
  });

  it("sin ningún código lo dice explícitamente y no devuelve visor vacío", () => {
    expect(codigoAMostrar(version({}))).toEqual({
      codigo: null,
      etiqueta: "built-in: sin código",
    });
  });

  it("un sourceCode NULL con code presente cae en la rama del JS etiquetado", () => {
    const js = "const x = 1;\n";
    expect(codigoAMostrar(version({ sourceCode: null, code: js })).codigo).toBe(js);
    expect(codigoAMostrar(version({ sourceCode: null, code: js })).etiqueta).toContain(
      "JS transpilado",
    );
  });

  it("un sourceCode presente prevalece aunque code sea null", () => {
    const ts = "export default () => 1;\n";
    const mostrado = codigoAMostrar(version({ sourceCode: ts, code: null }));
    expect(mostrado).toEqual({ codigo: ts, etiqueta: null });
  });

  it("un sourceCode vacío no cuenta como fuente: cae al JS etiquetado", () => {
    const js = "const x = 1;\n";
    const mostrado = codigoAMostrar(version({ sourceCode: "", code: js }));
    expect(mostrado.codigo).toBe(js);
    expect(mostrado.etiqueta).toContain("JS transpilado");
  });
});
