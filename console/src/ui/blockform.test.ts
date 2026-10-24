//! Tests del render lógico por widget (5.1): la decisión widget→control sin DOM.
import { describe, expect, it } from "vitest";
import { renderDeWidget } from "./blockform";

describe("renderDeWidget", () => {
  it("select con opciones del enum; sin opciones cae a texto (D1)", () => {
    expect(renderDeWidget("select", ["GET", "POST"])).toEqual({ tipo: "select" });
    expect(renderDeWidget("select", [])).toEqual({ tipo: "input", inputType: "text" });
    expect(renderDeWidget("select", undefined)).toEqual({ tipo: "input", inputType: "text" });
  });

  it("json y ref-template van a sus controles dedicados", () => {
    expect(renderDeWidget("json")).toEqual({ tipo: "json" });
    expect(renderDeWidget("ref-template")).toEqual({ tipo: "refs" });
  });

  it("textarea; ids de input tipado; desconocidos a text", () => {
    expect(renderDeWidget("textarea")).toEqual({ tipo: "textarea" });
    expect(renderDeWidget("url")).toEqual({ tipo: "input", inputType: "url" });
    expect(renderDeWidget("number")).toEqual({ tipo: "input", inputType: "number" });
    expect(renderDeWidget("password")).toEqual({ tipo: "input", inputType: "password" });
    expect(renderDeWidget("text")).toEqual({ tipo: "input", inputType: "text" });
    expect(renderDeWidget("kebab-inventado")).toEqual({ tipo: "input", inputType: "text" });
  });
});
