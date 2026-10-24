//! Tests G2: colapso de tarjetas (solo la última abierta por defecto; errores
//! fuerzan apertura) y quitar/reinsertar con Deshacer.
import { describe, expect, it } from "vitest";
import { estaExpandida, purgarAbiertas, quitarInstancia, reinsertarInstancia } from "./tarjetas-cadena";

describe("purgarAbiertas", () => {
  it("primera carga (0→4): solo la última abierta", () => {
    expect(purgarAbiertas([], 4)).toEqual([3]);
  });
  it("añadir una: se conserva lo abierto y la nueva última abre", () => {
    expect(purgarAbiertas([3], 5)).toEqual([3, 4]);
  });
  it("quitar una: índices fuera de rango desaparecen y la última abre", () => {
    expect(purgarAbiertas([0, 2, 4], 4)).toEqual([0, 2, 3]);
  });
  it("cadena vacía: nada abierto", () => {
    expect(purgarAbiertas([0, 1], 0)).toEqual([]);
  });
});

describe("quitarInstancia / reinsertarInstancia", () => {
  const lista = ["a", "b", "c"];
  it("quitar devuelve la quitada y las que quedan", () => {
    const r = quitarInstancia(lista, 1);
    expect(r.quitada).toBe("b");
    expect(r.quedan).toEqual(["a", "c"]);
    expect(lista).toEqual(["a", "b", "c"]); // inmutable
  });
  it("índice fuera de rango: no altera", () => {
    expect(quitarInstancia(lista, 9)).toEqual({ quedan: ["a", "b", "c"], quitada: undefined });
    expect(quitarInstancia(lista, -1).quitada).toBeUndefined();
  });
  it("deshacer reinserta en su posición original", () => {
    const r = quitarInstancia(lista, 1);
    expect(reinsertarInstancia(r.quedan, 1, r.quitada!)).toEqual(["a", "b", "c"]);
  });
  it("reinsertar acota el índice a los límites", () => {
    expect(reinsertarInstancia(["a"], 9, "b")).toEqual(["a", "b"]);
  });
});

describe("estaExpandida", () => {
  it("errores fuerzan apertura; sin ellos manda la lista abierta", () => {
    expect(estaExpandida(1, [0, 2], false)).toBe(false);
    expect(estaExpandida(2, [0, 2], false)).toBe(true);
    expect(estaExpandida(1, [0, 2], true)).toBe(true);
  });
});
