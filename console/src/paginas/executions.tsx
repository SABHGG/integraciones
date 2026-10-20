//! Página de ejecuciones del sistema El rastro (MVP6): banda de métricas,
//! serie diaria y el listado con el rastro en miniatura por fila. El detalle
//! presenta el trace como cadena (nudo + raíl) con IO en mono. Los datos y
//! la lógica provienen del MVP3+MVP5; el backend aditivo trae el resumen de
//! pasos sin IO.
import { useEffect, useRef, useState } from "preact/hooks";
import { api, type Page } from "../lib/api";
import { BandaKpis, DesglosePuentes, RANGOS, SerieDiaria, type Stats } from "./dashboard";
import { hace, crearPolling, INTERVALO_REFRESCO_MS } from "../lib/metricas";
import { Rastro, type PasoResumen } from "../ui";
import { Boton, Control, Esqueleto, Insignia, Panel } from "../ui";

interface Ejecucion {
  id: string;
  bridgeId: string;
  triggerType: string;
  status: string;
  error: { code?: string; message?: string } | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
  pasos?: PasoResumen[];
}

interface PuenteRef {
  id: string;
  slug: string;
  name: string;
}

/** Aviso de error de la página: qué pasó, en la voz del servidor (Problem). */
function AvisoError(props: { texto: string }) {
  return (
    <div class="rounded-[8px] border border-mal/40 bg-mal/5 px-3.5 py-2.5 text-[13.5px] text-mal">
      {props.texto}
    </div>
  );
}

export function Ejecuciones(props: { navegar: (ruta: string) => void }) {
  const [items, setItems] = useState<Ejecucion[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);
  const [puentes, setPuentes] = useState<PuenteRef[]>([]);
  const [puenteId, setPuenteId] = useState("");
  const [estado, setEstado] = useState("");
  const [desde, setDesde] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [autoRefresco, setAutoRefresco] = useState(false);
  const [ultima, setUltima] = useState<Date | null>(null);

  // Métricas: la página las posee y las reparte a la banda y a la serie.
  const [dias, setDias] = useState(30);
  const [stats, setStats] = useState<Stats | null>(null);
  const [cargandoStats, setCargandoStats] = useState(true);

  useEffect(() => {
    const to = new Date();
    const from = new Date(to.getTime() - dias * 24 * 3600 * 1000);
    setCargandoStats(true);
    void api<Stats>("GET", `/stats?from=${from.toISOString()}&to=${to.toISOString()}`)
      .then((s) => {
        setStats(s);
        setError(null);
      })
      .catch((e) => setError(e instanceof Error ? e.message : "error al cargar métricas"))
      .finally(() => setCargandoStats(false));
  }, [dias]);

  const filtros = () => {
    const parametros = new URLSearchParams();
    if (puenteId) parametros.set("bridgeId", puenteId);
    if (estado) parametros.set("status", estado);
    if (desde) parametros.set("from", new Date(desde).toISOString());
    return parametros;
  };

  const cargar = async () => {
    setError(null);
    try {
      const pagina = await api<Page<Ejecucion>>("GET", `/executions?${filtros()}`);
      setItems(pagina.items);
      setCursor(pagina.nextCursor);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar");
    } finally {
      setCargando(false);
    }
  };

  // El polling siempre llama a la última versión de cargar (filtros frescos).
  const cargarRef = useRef(cargar);
  cargarRef.current = cargar;

  useEffect(() => {
    if (!autoRefresco) return;
    const polling = crearPolling(INTERVALO_REFRESCO_MS, async () => {
      await cargarRef.current();
      setUltima(new Date());
    }, { estaOculto: () => document.hidden });
    polling.arrancar();
    return () => polling.parar();
  }, [autoRefresco]);

  useEffect(() => {
    void cargar();
    void api<Page<PuenteRef>>("GET", "/bridges?limit=100").then((pagina) => setPuentes(pagina.items));
    // Los filtros disparan la carga; el efecto inicial solo una vez.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [puenteId, estado, desde]);

  const cargarMas = async () => {
    if (!cursor) return;
    try {
      const pagina = await api<Page<Ejecucion>>(
        "GET",
        `/executions?${filtros()}&cursor=${encodeURIComponent(cursor)}`,
      );
      setItems([...items, ...pagina.items]);
      setCursor(pagina.nextCursor);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar");
    }
  };

  const slugDe = (bridgeId: string) =>
    puentes.find((p) => p.id === bridgeId)?.slug ?? bridgeId.slice(0, 8);

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-5 gap-y-2">
        <h1 class="text-[22px] font-bold tracking-tight text-tinta">Ejecuciones</h1>
        <span class="text-[13.5px] text-tinta-suave">Rastros de invocación de tus puentes</span>
        <div class="ml-auto flex flex-wrap items-center gap-2">
          <div class="flex overflow-hidden rounded-[7px] border border-borde-fuerte bg-superficie" role="group" aria-label="Rango de fechas">
            {RANGOS.map((rango) => (
              <button
                key={rango.dias}
                class={`px-2 py-1.5 text-[12px] font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-tinta ${dias === rango.dias ? "bg-tinta text-papel" : "text-tinta-suave hover:text-tinta"}`}
                onClick={() => setDias(rango.dias)}
              >
                {rango.etiqueta}
              </button>
            ))}
          </div>
          <Control as="select" class="w-auto!" value={estado} onChange={(e: Event) => setEstado((e.target as HTMLSelectElement).value)} aria-label="Filtrar por estado">
            <option value="">todos los estados</option>
            <option value="pending">pending</option>
            <option value="running">running</option>
            <option value="succeeded">succeeded</option>
            <option value="failed">failed</option>
          </Control>
          <Control as="select" class="w-auto! max-w-[200px]" value={puenteId} onChange={(e: Event) => setPuenteId((e.target as HTMLSelectElement).value)} aria-label="Filtrar por puente">
            <option value="">todos los puentes</option>
            {puentes.map((p) => (
              <option key={p.id} value={p.id}>{p.slug}</option>
            ))}
          </Control>
        </div>
      </div>

      {error && <AvisoError texto={error} />}
      {cargandoStats || !stats ? (
        <Panel><Esqueleto filas={3} /></Panel>
      ) : (
        <>
          <BandaKpis stats={stats} dias={dias} />
          <SerieDiaria stats={stats} />
        </>
      )}

      <Panel class="overflow-hidden">
        {/* Filtros de listado fuera de la cabecera del target: el rango fino
            y el auto-refresh viven pegados a la tabla que afectan. */}
        <div class="flex flex-wrap items-center gap-3 border-b border-borde px-4 py-2">
          <Control type="datetime-local" class="w-[175px]!" value={desde} onChange={(e: Event) => setDesde((e.target as HTMLInputElement).value)} aria-label="Desde" />
          <label class="ml-auto flex items-center gap-1.5 text-[12.5px] text-tinta-suave" title="Cada 5 s; se pausa con la pestaña oculta">
            <input
              type="checkbox"
              class="h-4 w-4 accent-[#14352d]"
              checked={autoRefresco}
              onChange={(e) => setAutoRefresco((e.target as HTMLInputElement).checked)}
            />
            Auto-refresh
            {ultima && <span class="font-mono text-[11px] text-tinta-tenue">{ultima.toLocaleTimeString()}</span>}
          </label>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full min-w-[860px] border-collapse">
            <thead>
              <tr class="border-b border-borde bg-superficie text-left text-[12px] font-semibold text-tinta-suave">
                <th class="w-[126px] px-4 py-2.5 font-semibold">Estado</th>
                <th class="w-[150px] px-4 py-2.5 font-semibold">Puente</th>
                <th class="w-[200px] px-4 py-2.5 font-semibold">Rastro</th>
                <th class="w-[80px] px-4 py-2.5 font-semibold">Trigger</th>
                <th class="w-[110px] px-4 py-2.5 font-semibold">Creada</th>
                <th class="px-4 py-2.5 font-semibold">Resultado</th>
              </tr>
            </thead>
            <tbody>
              {items.map((ejecucion) => (
                <tr
                  key={ejecucion.id}
                  class="cursor-pointer border-b border-borde transition-colors last:border-0 hover:bg-tinta/3 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-tinta"
                  tabIndex={0}
                  onClick={() => props.navegar(`/executions/${ejecucion.id}`)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") props.navegar(`/executions/${ejecucion.id}`);
                  }}
                >
                  <td class="px-4 py-2"><Insignia estado={ejecucion.status} /></td>
                  <td class="px-4 py-2 font-mono text-[12.5px]">{slugDe(ejecucion.bridgeId)}</td>
                  <td class="px-4 py-2"><Rastro pasos={ejecucion.pasos} estado={ejecucion.status} /></td>
                  <td class="px-4 py-2 font-mono text-[12.5px] text-tinta-suave">{ejecucion.triggerType}</td>
                  <td class="px-4 py-2 text-[13px] text-tinta-suave" title={new Date(ejecucion.createdAt).toLocaleString()}>
                    {hace(ejecucion.createdAt)}
                  </td>
                  <td class="px-4 py-2">
                    {ejecucion.error?.code && (
                      <span class="block max-w-[300px] truncate font-mono text-[12px] text-mal" title={ejecucion.error?.message ?? ""}>
                        {ejecucion.error.code}: {ejecucion.error.message}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
              {!cargando && items.length === 0 && !error && (
                <tr>
                  <td colspan={6} class="px-4 py-8 text-center text-[13.5px] text-tinta-suave">
                    Sin ejecuciones con esos filtros. Ajusta los filtros o dispara un puente.
                  </td>
                </tr>
              )}
              {cargando && items.length === 0 && (
                <tr><td colspan={6}><Esqueleto filas={4} /></td></tr>
              )}
            </tbody>
          </table>
        </div>
        {cursor && (
          <div class="border-t border-borde px-4 py-3">
            <Boton variante="secundario" chico onClick={cargarMas}>Cargar más</Boton>
          </div>
        )}
      </Panel>

      {stats && <DesglosePuentes stats={stats} alFiltrarPuente={setPuenteId} />}
    </div>
  );
}

export interface Paso {
  seq: number;
  blockName: string;
  status: string;
  input: unknown;
  output: unknown;
  error: { message?: string } | null;
  durationMs: number | null;
}

/** Detalle de ejecución: el rastro completo como cadena de nudos con IO. */
export function DetalleEjecucion(props: { id: string }) {
  const [detalle, setDetalle] = useState<Record<string, unknown> | null>(null);
  const [pasos, setPasos] = useState<Paso[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        setDetalle(await api("GET", `/executions/${props.id}`));
        const trace = await api<{ steps: Paso[] }>("GET", `/executions/${props.id}/trace`);
        setPasos(trace.steps);
      } catch (e) {
        setError(e instanceof Error ? e.message : "error al cargar");
      }
    })();
  }, [props.id]);

  if (error) return <AvisoError texto={error} />;
  if (!detalle) return <Panel><Esqueleto filas={4} /></Panel>;

  const estado = String(detalle.status);
  const detalleError = detalle.error as { code?: string; message?: string } | null;
  const replays = (detalle.replays as unknown[] | undefined)?.length ?? 0;

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="text-[22px] font-bold tracking-tight text-tinta">Ejecución</h1>
        <Insignia estado={estado} />
        <code class="font-mono text-[12.5px] text-tinta-suave">{props.id}</code>
        {replays > 0 && (
          <span class="text-[12.5px] text-tinta-suave">{replays} {replays === 1 ? "replay" : "replays"}</span>
        )}
      </div>

      {detalleError && (
        <AvisoError texto={`${detalleError.code ?? "error"}: ${detalleError.message ?? "sin detalle"}`} />
      )}

      {detalle.triggerData !== undefined && (
        <Panel class="p-4">
          <div class="mb-2 text-[13px] font-medium text-tinta">Entrada del trigger</div>
          <p class="mb-2 text-[12px] text-tinta-tenue">Enmascarada si el puente aplica mask sobre este campo.</p>
          <pre class="max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2.5 font-mono text-[11.5px] leading-relaxed">
            {JSON.stringify(detalle.triggerData, null, 2)}
          </pre>
        </Panel>
      )}

      <Panel class="p-5">
        <div class="mb-3 text-[14px] font-semibold text-tinta">Rastro paso a paso</div>
        {pasos.length === 0 ? (
          <p class="text-[13.5px] text-tinta-suave">La ejecución aún no tiene pasos.</p>
        ) : (
          <ol>
            {pasos.map((paso, i) => (
              <li
                key={paso.seq}
                class={`relative ml-[7px] border-l-2 pl-7 ${i === pasos.length - 1 ? "pb-1" : "pb-5"} ${paso.status === "skipped" ? "border-borde" : paso.status === "failed" ? "border-mal" : "border-borde-fuerte"}`}
              >
                <span
                  class={`absolute -left-[9px] top-0.5 h-4 w-4 rounded-full border-2 bg-superficie ${paso.status === "skipped" ? "border-borde" : paso.status === "failed" ? "border-mal bg-mal-fondo" : "border-borde-fuerte"}`}
                  aria-hidden="true"
                />
                <div class="flex flex-wrap items-center gap-2.5">
                  <span class="font-mono text-[12.5px] font-semibold text-tinta">{paso.blockName}</span>
                  <Insignia estado={paso.status} />
                  {paso.durationMs !== null && (
                    <span class="font-mono text-[11.5px] text-tinta-suave">{paso.durationMs} ms</span>
                  )}
                </div>
                {paso.error && (
                  <p class="mt-1.5 rounded-[6px] border border-mal/40 bg-mal/5 px-2.5 py-1.5 text-[12.5px] text-mal">
                    {paso.error.message}
                  </p>
                )}
                {paso.status !== "skipped" && (
                  <details class="mt-2">
                    <summary class="cursor-pointer text-[12px] text-tinta-suave hover:text-tinta">
                      Entrada / salida (enmascaradas si aplica)
                    </summary>
                    <pre class="mt-2 max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2.5 font-mono text-[11.5px] leading-relaxed">
                      {JSON.stringify(paso.input, null, 2)}
                    </pre>
                    <pre class="mt-1.5 max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2.5 font-mono text-[11.5px] leading-relaxed">
                      {JSON.stringify(paso.output, null, 2)}
                    </pre>
                  </details>
                )}
              </li>
            ))}
          </ol>
        )}
      </Panel>
    </div>
  );
}
