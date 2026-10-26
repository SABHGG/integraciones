//! Guardia de salida (critique R2 P1): una sola fuente por página declara
//! trabajo sin guardar; el router (navegar/popstate) consulta antes de mover.
//! `permitirSalidaUnaVez` evita el doble confirm cuando `navegar` ya preguntó
//! y la navegación sigue por popstate.

export type GuardaSalida = () => string | null;

let guardia: GuardaSalida | null = null;
let exencion = false;

/** La pantalla activa declara su trabajo pendiente (mensaje) o se retira (null). */
export function ponerGuardia(g: GuardaSalida | null): void {
  guardia = g;
}

/** Mensaje de bloqueo si la pantalla declaró trabajo sin guardar; null si no. */
export function mensajeSalida(): string | null {
  return guardia ? guardia() : null;
}

/** Concede PASO libre a la próxima acción de navegación (el confirm ya fue). */
export function permitirSalidaUnaVez(): void {
  exencion = true;
}

/** Consume (una vez) la exención concedida. */
export function salidaPermitida(): boolean {
  const p = exencion;
  exencion = false;
  return p;
}

/** ¿Hay que preguntar antes de salir? (exención ya consumida → no). */
export function preguntaDeSalida(): string | null {
  if (salidaPermitida()) return null;
  return mensajeSalida();
}

/** Atajo pedido por pantallas: ¿hay guardia activa con mensaje? */
export function hayTrabajoSinGuardar(): boolean {
  return mensajeSalida() !== null;
}
