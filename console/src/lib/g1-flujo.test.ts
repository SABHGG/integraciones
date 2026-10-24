//! Tests G1: limpiarTrigger (sin huérfanos al cambiar tipo), parseSeguro
//! (parseo visible, nunca silencioso) y resumenPublicacion (confirmación).
import { describe, expect, it } from "vitest";
import { limpiarTrigger } from "./validacion-trigger";
import { parseSeguro, textoDe } from "./parse-seguro";
import { resumenPublicacion } from "./resumen-publicacion";

describe("limpiarTrigger", () => {
  it("http→cron deja method/path huérfanos fuera y conserva lo del nuevo tipo", () => {
    const limpio = limpiarTrigger("cron", { type: "http", method: "POST", path: "/x/a", schedule: "*/5 * * * *" });
    expect(limpio).toEqual({ type: "cron", schedule: "*/5 * * * *" });
  });
  it("cron→http conserva method/path si ya estaban; manual deja solo type", () => {
    expect(limpiarTrigger("http", { type: "cron", schedule: "x", method: "GET", path: "/x/b" })).toEqual({
      type: "http", method: "GET", path: "/x/b",
    });
    expect(limpiarTrigger("manual", { type: "event", topic: "a.b" })).toEqual({ type: "manual" });
  });
  it("tipo desconocido: solo type (el servidor reportará el tipo inválido)", () => {
    expect(limpiarTrigger("rss", { type: "http", method: "GET" })).toEqual({ type: "rss" });
  });
});

describe("parseSeguro", () => {
  it("vacío → ok undefined (campo limpio, no error)", () => {
    expect(parseSeguro("")).toEqual({ ok: true, valor: undefined });
    expect(parseSeguro("   \n")).toEqual({ ok: true, valor: undefined });
  });
  it("JSON válido → valor estructurado", () => {
    expect(parseSeguro('{"a": 1}')).toEqual({ ok: true, valor: { a: 1 } });
    expect(parseSeguro('[1,2]')).toEqual({ ok: true, valor: [1, 2] });
  });
  it("inválido → error legible, nunca silencio", () => {
    const r = parseSeguro('{"a": ');
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toContain("JSON inválido");
  });
});

describe("textoDe", () => {
  it("undefined → ''; el resto → JSON indentado", () => {
    expect(textoDe(undefined)).toBe("");
    expect(textoDe({ a: 1 })).toBe('{\n  "a": 1\n}');
  });
});

describe("resumenPublicacion", () => {
  it("http: verbo + path; cron: schedule + zona; event: topic; manual", () => {
    expect(resumenPublicacion({ type: "http", method: "POST", path: "/x/demo" }, [{ name: "a" }])).toBe("POST /x/demo · 1 bloque");
    expect(resumenPublicacion({ type: "cron", schedule: "*/5 * * * *", timezone: "UTC" }, [{ name: "a" }, { name: "b" }])).toBe("cron */5 * * * * (UTC) · 2 bloques");
    expect(resumenPublicacion({ type: "event", topic: "pedidos.creado" }, [])).toBe("evento pedidos.creado · 0 bloques");
    expect(resumenPublicacion({ type: "manual" }, [{ name: "a" }])).toBe("manual · 1 bloque");
  });
  it("trigger ausente o a medias: lo dice sin inventar", () => {
    expect(resumenPublicacion(undefined, [])).toBe("disparador sin definir · 0 bloques");
    expect(resumenPublicacion({ type: "http", method: "POST" }, [])).toBe("disparador http · 0 bloques");
  });
});
