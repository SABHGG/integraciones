//! El rastro en miniatura (MVP6): la firma del sistema visual. Cada
//! ejecución del listado muestra un segmento por paso, proporcional a su
//! duración real y coloreado por estado — ver qué bloque es lento o falló
//! sin abrir el detalle. Datos del resumen aditivo de `GET /executions`
//! (estado + duración por paso, sin IO).
import { formatearDuracion } from "../lib/metricas";

export interface PasoResumen {
  status: string;
  durationMs: number | null;
}

const CLASE_SEGMENTO: Record<string, string> = {
  succeeded: "bg-ok",
  failed: "bg-mal",
  running: "bg-info",
  skipped: "bg-aviso/45",
  pending: "bg-aviso/45",
};

/** Ancho mínimo de un segmento (px equivalentes vía flex-basis) para que
 los pasos relámpago sigan siendo visibles junto a un paso de segundos. */
const FLEX_MINIMO = 2;

export function Rastro(props: { pasos?: PasoResumen[]; estado: string }) {
  const pasos = props.pasos ?? [];

  if (pasos.length === 0) {
    // Ejecución en cola o arrancando: sin segmentos, lo decimos.
    return (
      <div class="rastro">
        <div class="font-mono text-[11px] text-tinta-tenue">
          {props.estado === "pending" ? "en cola" : "sin pasos aún"}
        </div>
      </div>
    );
  }

  const totalMs = pasos.reduce((suma, paso) => suma + (paso.durationMs ?? 0), 0);
  const corriendo = props.estado === "running";
  const resumen = corriendo
    ? `en curso · ${formatearDuracion(totalMs)}`
    : `${pasos.length} ${pasos.length === 1 ? "paso" : "pasos"} · ${formatearDuracion(totalMs)}`;

  return (
    <div class="rastro" aria-label={`Rastro: ${resumen}`}>
      <div class="flex h-[7px] items-stretch gap-[1.5px]">
        {pasos.map((paso, i) => (
          <span
            key={i}
            class={`min-w-[2px] rounded-[1.5px] ${CLASE_SEGMENTO[paso.status] ?? "bg-tinta-tenue/40"}`}
            style={`flex: ${paso.durationMs ?? FLEX_MINIMO} 1 0%`}
            title={`${paso.status}${paso.durationMs !== null ? ` · ${paso.durationMs} ms` : ""}`}
          />
        ))}
      </div>
      <div class="mt-0.5 font-mono text-[11px] leading-[14px] text-tinta-suave">{resumen}</div>
    </div>
  );
}
