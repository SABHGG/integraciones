//! Tests de la guardia de salida (F1): registro, mensaje y exención de una vez.
import { describe, expect, it } from "vitest";
import { hayTrabajoSinGuardar, mensajeSalida, permitirSalidaUnaVez, ponerGuardia, preguntaDeSalida, salidaPermitida } from "./nav-guarda";

describe("guardia de salida", () => {
  it("sin guardia: no hay mensaje ni pregunta", () => {
    ponerGuardia(null);
    expect(mensajeSalida()).toBeNull();
    expect(preguntaDeSalida()).toBeNull();
    expect(hayTrabajoSinGuardar()).toBe(false);
  });

  it("con dirty: mensaje; limpio: null", () => {
    let dirty = true;
    ponerGuardia(() => (dirty ? "Hay cambios sin guardar" : null));
    expect(mensajeSalida()).toBe("Hay cambios sin guardar");
    dirty = false;
    expect(preguntaDeSalida()).toBeNull();
  });

  it("permitirSalidaUnaVez consume una sola vez", () => {
    ponerGuardia(() => "bloquea");
    permitirSalidaUnaVez();
    expect(salidaPermitida()).toBe(true);
    expect(salidaPermitida()).toBe(false);
    expect(preguntaDeSalida()).toBe("bloquea");
  });
});
