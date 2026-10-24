//! Tests de las funciones puras del autocompletado (filtrado y fragmento);
//! la interacción DOM del dropdown se verifica a mano con `npm run dev`.
import { describe, expect, it } from "vitest";
import { filtrarSugerencias, fragmentoEnCursor } from "./input-con-refs";
import type { SugerenciaRef } from "../lib/refs-contexto";

const s = (...rutas: string[]): SugerenciaRef[] =>
  rutas.map((ruta) => ({ ruta, fuente: ruta.split(".")[1] ?? "?" }));

describe("fragmentoEnCursor", () => {
  it("devuelve la cola de caracteres de ruta antes del cursor", () => {
    expect(fragmentoEnCursor("$.usuario.id", 12)).toBe("$.usuario.id");
    expect(fragmentoEnCursor("$.us", 4)).toBe("$.us");
    expect(fragmentoEnCursor('{"a": 1, "b": $', 15)).toBe("$");
  });
  it("el fragmento se corta en caracteres que no son de ruta", () => {
    expect(fragmentoEnCursor('{"url": "https://x/{$.us', 24)).toBe("$.us");
    expect(fragmentoEnCursor("hola mundo", 10)).toBe("mundo");
  });
  it("cursor 0: fragmento vacío", () => {
    expect(fragmentoEnCursor("$.x", 0)).toBe("");
  });
});

describe("filtrarSugerencias", () => {
  const lista = s("$.trigger.body", "$.trigger.query", "$.now", "$.usuario.id");
  it("sin fragmento: todo en orden", () => {
    expect(filtrarSugerencias(lista, "").visibles).toEqual(lista);
  });
  it("filtra por substring sin mayúsculas", () => {
    expect(filtrarSugerencias(lista, "trigger").visibles.map((x) => x.ruta)).toEqual([
      "$.trigger.body", "$.trigger.query",
    ]);
    expect(filtrarSugerencias(lista, "USUARIO").visibles.map((x) => x.ruta)).toEqual(["$.usuario.id"]);
  });
  it("sin coincidencias: lista vacía", () => {
    expect(filtrarSugerencias(lista, "zzz")).toEqual({ visibles: [], ocultas: 0 });
  });
  it("acota al máximo y cuenta las ocultas", () => {
    const muchas = s(...Array.from({ length: 12 }, (_, i) => `$.ruta${i}`));
    const r = filtrarSugerencias(muchas, "", 8);
    expect(r.visibles).toHaveLength(8);
    expect(r.ocultas).toBe(4);
  });
});
