//! Métricas del dashboard (MVP5) presentadas con el sistema El rastro (MVP6):
//! banda de indicadores con divisores (sin tarjetas), serie diaria apilada
//! correctas/fallidas y desglose por puente navegable al listado filtrado.
//! La obtención de datos vive en la página; esto es presentación pura.
import { formatearDuracion, picoSerie, rellenarSerie, type DiaCompleto } from "./metricas";
import { Boton, Panel } from "./ui";

export interface Stats {
  from: string;
  to: string;
  total: number;
  byStatus: Record<string, number>;
  avgDurationMs: number | null;
  daily: { day: string; total: number; succeeded: number; failed: number }[];
  bridges: {
    items: {
      bridgeId: string;
      slug: string;
      total: number;
      succeeded: number;
      failed: number;
      avgDurationMs: number | null;
    }[];
    nextCursor: string | null;
  };
}

export const RANGOS = [
  { dias: 7, etiqueta: "7 días" },
  { dias: 30, etiqueta: "30 días" },
  { dias: 90, etiqueta: "90 días" },
];

export function selectorSerie(stats: Stats): DiaCompleto[] {
  const diaISO = (fecha: Date) => fecha.toISOString().slice(0, 10);
  return rellenarSerie(diaISO(new Date(stats.from)), diaISO(new Date(stats.to)), stats.daily);
}

function porcentaje(parte: number, total: number): string {
  if (total === 0) return "—";
  return `${((parte / total) * 100).toFixed(1).replace(".", ",")} %`;
}

/** Banda de indicadores con divisores verticales: la salud de un vistazo. */
export function BandaKpis(props: { stats: Stats; dias: number }) {
  const { stats } = props;
  const exitosas = stats.byStatus.succeeded ?? 0;
  const fallidas = stats.byStatus.failed ?? 0;
  const kpis = [
    {
      etiqueta: "Ejecuciones",
      cifra: <>{stats.total.toLocaleString("es")}</>,
      detalle: `últimos ${props.dias} días`,
      tono: "",
    },
    {
      etiqueta: "Éxito",
      cifra: <>{porcentaje(exitosas, stats.total)}</>,
      detalle: `${exitosas.toLocaleString("es")} correctas`,
      tono: "text-ok",
    },
    {
      etiqueta: "Fallos",
      cifra: <>{fallidas.toLocaleString("es")}</>,
      detalle: `${porcentaje(fallidas, stats.total)} del total`,
      tono: fallidas > 0 ? "text-mal" : "",
    },
    {
      etiqueta: "Duración media",
      cifra: <>{formatearDuracion(stats.avgDurationMs)}</>,
      detalle: "media de exitosas",
      tono: "",
    },
  ];

  return (
    <div class="grid grid-cols-2 overflow-hidden rounded-panel border border-borde bg-superficie md:grid-cols-4">
      {kpis.map((kpi, i) => (
        <div class={`p-4 pl-5 ${i > 0 ? "border-borde max-md:border-t max-md:odd:border-l md:border-l" : ""}`}>
          <div class="text-[13px] text-tinta-suave">{kpi.etiqueta}</div>
          <div class={`mt-1 mb-0.5 font-mono text-[24px] font-semibold leading-tight tracking-tight tabular-nums text-tinta ${kpi.tono}`}>
            {kpi.cifra}
          </div>
          <div class="text-[12px] text-tinta-tenue">{kpi.detalle}</div>
        </div>
      ))}
    </div>
  );
}

/** Serie diaria apilada: correctas (verde) y fallidas (ladrillo). */
export function SerieDiaria(props: { stats: Stats }) {
  const serie = selectorSerie(props.stats);
  const pico = picoSerie(serie);
  const alturaMax = 96;
  const hoy = new Date().toISOString().slice(0, 10);
  const etiquetas = [0, 0.25, 0.5, 0.75, 1].map((fraccion) => {
    const indice = Math.min(serie.length - 1, Math.round(fraccion * (serie.length - 1)));
    const dia = serie[indice];
    const atras = Math.round(
      (new Date(`${hoy}T00:00:00Z`).getTime() - new Date(`${dia.day}T00:00:00Z`).getTime()) / 86400000,
    );
    return atras <= 0 ? "hoy" : `hace ${atras} d`;
  });

  return (
    <Panel class="px-4 pb-2.5 pt-3.5">
      <div class="flex items-baseline gap-4">
        <span class="text-[14px] font-semibold text-tinta">Serie diaria</span>
        <div class="ml-auto flex gap-3.5 text-[12.5px] text-tinta-suave">
          <span><span class="mr-1.5 inline-block h-2 w-2.5 rounded-[2px] bg-ok align-[-1px]" />correctas</span>
          <span><span class="mr-1.5 inline-block h-2 w-2.5 rounded-[2px] bg-mal align-[-1px]" />fallidas</span>
        </div>
      </div>
      <div class="mt-3 flex h-24 items-end gap-1">
        {serie.map((dia) => (
          <div
            key={dia.day}
            class="flex min-w-[2px] flex-1 flex-col justify-end gap-[1.5px]"
            title={`${dia.day}: ${dia.total} (${dia.succeeded} correctas, ${dia.failed} fallidas)`}
          >
            {dia.failed > 0 && (
              <span
                class="w-full rounded-t-[1.5px] bg-mal"
                style={`height: ${Math.max(2, Math.round((dia.failed / pico) * alturaMax))}px`}
              />
            )}
            {dia.succeeded > 0 && (
              <span
                class="w-full rounded-t-[1.5px] bg-ok"
                style={`height: ${Math.max(2, Math.round((dia.succeeded / pico) * alturaMax))}px`}
              />
            )}
          </div>
        ))}
      </div>
      <div class="mt-1.5 flex justify-between border-t border-borde pt-1.5 font-mono text-[10.5px] text-tinta-tenue">
        {etiquetas.map((etiqueta, i) => (
          <span key={i}>{etiqueta}</span>
        ))}
      </div>
    </Panel>
  );
}

/** Desglose por puente: navegable al listado filtrado (spec MVP5). */
export function DesglosePuentes(props: { stats: Stats; alFiltrarPuente: (bridgeId: string) => void }) {
  const { stats } = props;
  return (
    <Panel>
      <div class="px-4 pt-3.5 text-[14px] font-semibold text-tinta">Desglose por puente</div>
      <div class="overflow-x-auto">
      <table class="mt-2 w-full min-w-[560px] border-collapse">
        <thead>
          <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
            <th class="px-4 py-2 font-semibold">Puente</th>
            <th class="px-4 py-2 font-semibold">Total</th>
            <th class="px-4 py-2 font-semibold">Correctas</th>
            <th class="px-4 py-2 font-semibold">Fallidas</th>
            <th class="px-4 py-2 font-semibold">Media</th>
            <th class="px-4 py-2" />
          </tr>
        </thead>
        <tbody>
          {stats.bridges.items.map((puente) => (
            <tr key={puente.bridgeId} class="border-b border-borde last:border-0">
              <td class="px-4 py-2.5 font-mono text-[12.5px]">{puente.slug}</td>
              <td class="px-4 py-2.5 text-[13.5px] tabular-nums">{puente.total}</td>
              <td class="px-4 py-2.5 text-[13.5px] tabular-nums text-ok">{puente.succeeded}</td>
              <td class="px-4 py-2.5 text-[13.5px] tabular-nums">{puente.failed > 0 ? puente.failed : ""}</td>
              <td class="px-4 py-2.5 font-mono text-[12.5px] text-tinta-suave">{formatearDuracion(puente.avgDurationMs)}</td>
              <td class="px-4 py-2.5 text-right">
                <Boton variante="secundario" chico onClick={() => props.alFiltrarPuente(puente.bridgeId)}>
                  Ver ejecuciones
                </Boton>
              </td>
            </tr>
          ))}
          {stats.bridges.items.length === 0 && (
            <tr>
              <td colspan={6} class="px-4 py-4 text-[13.5px] text-tinta-suave">
                Sin ejecuciones en el rango elegido.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      </div>
    </Panel>
  );
}
