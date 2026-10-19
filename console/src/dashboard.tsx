//! Métricas del dashboard (MVP5, tasks 5.1-5.2): total y estados, serie
//! diaria con huecos rellenados, duración media y desglose por puente
//! navegable al listado filtrado. Lectura: cualquier rol la ve (D8).
import { useEffect, useState } from "preact/hooks";
import { api } from "./api";
import {
  rellenarSerie,
  formatearDuracion,
  picoSerie,
  type DiaCompleto,
} from "./metricas";

interface Stats {
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

const RANGOS = [
  { dias: 7, etiqueta: "7 días" },
  { dias: 30, etiqueta: "30 días" },
  { dias: 90, etiqueta: "90 días" },
];

export function Metricas(props: { alFiltrarPuente: (bridgeId: string) => void }) {
  const [dias, setDias] = useState(30);
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const to = new Date();
    const from = new Date(to.getTime() - dias * 24 * 3600 * 1000);
    setError(null);
    void api<Stats>("GET", `/stats?from=${from.toISOString()}&to=${to.toISOString()}`)
      .then(setStats)
      .catch((e) => setError(e instanceof Error ? e.message : "error al cargar métricas"));
  }, [dias]);

  if (error) return <p class="error-global">{error}</p>;
  if (!stats) return <p class="ayuda">cargando métricas…</p>;

  const diaISO = (fecha: Date) => fecha.toISOString().slice(0, 10);
  const serie: DiaCompleto[] = rellenarSerie(
    diaISO(new Date(stats.from)),
    diaISO(new Date(stats.to)),
    stats.daily,
  );
  const pico = picoSerie(serie);
  const estado = (clave: string) => stats.byStatus[clave] ?? 0;

  return (
    <div class="tarjeta">
      <div class="acciones" style="justify-content: space-between;">
        <h3 style="margin: 0;">salud del workspace</h3>
        <div class="acciones">
          {RANGOS.map((rango) => (
            <button
              key={rango.dias}
              class={dias === rango.dias ? "" : "secundario"}
              onClick={() => setDias(rango.dias)}
            >
              {rango.etiqueta}
            </button>
          ))}
        </div>
      </div>

      <div class="acciones" style="align-items: stretch;">
        <div class="metrica">
          <strong>{stats.total}</strong>
          <span class="ayuda">total</span>
        </div>
        <div class="metrica">
          <strong>{estado("succeeded")}</strong>
          <span class="ayuda">succeeded</span>
        </div>
        <div class="metrica">
          <strong>{estado("failed")}</strong>
          <span class="ayuda">failed</span>
        </div>
        <div class="metrica">
          <strong>{estado("running")}</strong>
          <span class="ayuda">running</span>
        </div>
        <div class="metrica">
          <strong>{estado("pending")}</strong>
          <span class="ayuda">pending</span>
        </div>
        <div class="metrica">
          <strong>{formatearDuracion(stats.avgDurationMs)}</strong>
          <span class="ayuda">media (exitosas)</span>
        </div>
      </div>

      <h4>tendencia diaria</h4>
      <div class="barras">
        {serie.map((dia) => (
          <div key={dia.day} class="barra" title={`${dia.day}: ${dia.total}`}>
            <div
              class="barra-relleno"
              style={`height: ${Math.round((dia.total / pico) * 60)}px;`}
            />
          </div>
        ))}
      </div>
      <p class="ayuda">
        {serie[0]?.day} → {serie[serie.length - 1]?.day} (los días sin
        ejecuciones van a cero)
      </p>

      <h4>desglose por puente</h4>
      <table>
        <thead>
          <tr>
            <th>puente</th><th>total</th><th>exitosas</th><th>fallidas</th><th>media</th><th></th>
          </tr>
        </thead>
        <tbody>
          {stats.bridges.items.map((puente) => (
            <tr key={puente.bridgeId}>
              <td>{puente.slug}</td>
              <td>{puente.total}</td>
              <td>{puente.succeeded}</td>
              <td>{puente.failed}</td>
              <td>{formatearDuracion(puente.avgDurationMs)}</td>
              <td>
                <button
                  class="secundario"
                  onClick={() => props.alFiltrarPuente(puente.bridgeId)}
                >
                  ver ejecuciones
                </button>
              </td>
            </tr>
          ))}
          {stats.bridges.items.length === 0 && (
            <tr><td colspan={6}>sin ejecuciones en el rango</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
