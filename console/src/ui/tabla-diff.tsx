//! Tabla del diff estructural (add-publish-diff T2, design D3): extraída de
//! PanelVersiones para compartirla con la confirmación de publicación.
//! Reglas de El rastro: rutas y valores en mono (valores de máquina),
//! etiquetas de lado en grotesca, `AUSENTE` atenuado y SIN croma de estado —
//! el diff no es un error: es información.
import type { LineaDiff } from "../lib/diff";
import { AUSENTE } from "../lib/diff";

export function TablaDiff(props: {
  lineas: LineaDiff[];
  etiquetaIzq: string;
  etiquetaDer: string;
}) {
  if (props.lineas.length === 0) return null;
  return (
    <div class="overflow-x-auto rounded-[6px] border border-borde">
      <table class="w-full min-w-[560px] border-collapse">
        <thead>
          <tr class="border-b border-borde bg-papel text-left text-[11.5px] font-semibold text-tinta-suave">
            <th class="px-3 py-2 font-semibold">Ruta</th>
            <th class="px-3 py-2 font-semibold">{props.etiquetaIzq}</th>
            <th class="px-3 py-2 font-semibold">{props.etiquetaDer}</th>
          </tr>
        </thead>
        <tbody>
          {props.lineas.map((linea) => (
            <tr key={linea.ruta} class="border-b border-borde last:border-0 align-top">
              <td class="px-3 py-2 font-mono text-[11.5px] text-tinta">{linea.ruta}</td>
              <td
                class={`max-w-[320px] truncate px-3 py-2 font-mono text-[11.5px] ${linea.izq === AUSENTE ? "text-tinta-tenue" : "text-tinta-suave"}`}
                title={JSON.stringify(linea.izq)}
              >
                {JSON.stringify(linea.izq)}
              </td>
              <td
                class={`max-w-[320px] truncate px-3 py-2 font-mono text-[11.5px] ${linea.der === AUSENTE ? "text-tinta-tenue" : "text-tinta"}`}
                title={JSON.stringify(linea.der)}
              >
                {JSON.stringify(linea.der)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
