//! Dashboard de ejecuciones: listado con filtros y cursor, detalle con
//! trace paso a paso (incluye `skipped`), IO ya enmascarado por el servidor.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";

interface Ejecucion {
  id: string;
  bridgeId: string;
  triggerType: string;
  status: string;
  error: { code?: string; message?: string } | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
}

interface PuenteRef {
  id: string;
  slug: string;
  name: string;
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

export function Ejecuciones(props: { navegar: (ruta: string) => void }) {
  const [items, setItems] = useState<Ejecucion[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [puentes, setPuentes] = useState<PuenteRef[]>([]);
  const [puenteId, setPuenteId] = useState("");
  const [estado, setEstado] = useState("");
  const [desde, setDesde] = useState("");
  const [error, setError] = useState<string | null>(null);

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
    }
  };

  useEffect(() => {
    void cargar();
    void api<Page<PuenteRef>>("GET", "/bridges?limit=100").then((pagina) => setPuentes(pagina.items));
    // Los filtros disparan la carga; el efecto inicial solo una vez.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [puenteId, estado, desde]);

  const cargarMas = async () => {
    if (!cursor) return;
    const pagina = await api<Page<Ejecucion>>(
      "GET",
      `/executions?${filtros()}&cursor=${encodeURIComponent(cursor)}`,
    );
    setItems([...items, ...pagina.items]);
    setCursor(pagina.nextCursor);
  };

  return (
    <div>
      <h1>Ejecuciones</h1>
      <div class="tarjeta" style="display: flex; gap: 10px; flex-wrap: wrap;">
        <select value={puenteId} onChange={(e) => setPuenteId((e.target as HTMLSelectElement).value)} style="max-width: 220px;">
          <option value="">todos los puentes</option>
          {puentes.map((p) => (
            <option value={p.id}>{p.slug}</option>
          ))}
        </select>
        <select value={estado} onChange={(e) => setEstado((e.target as HTMLSelectElement).value)} style="max-width: 160px;">
          <option value="">todos los estados</option>
          <option value="pending">pending</option>
          <option value="running">running</option>
          <option value="succeeded">succeeded</option>
          <option value="failed">failed</option>
        </select>
        <input type="datetime-local" value={desde} onChange={(e) => setDesde((e.target as HTMLInputElement).value)} style="max-width: 220px;" />
      </div>
      {error && <p class="error-global">{error}</p>}
      <div class="tarjeta">
        <table>
          <thead>
            <tr>
              <th>estado</th>
              <th>trigger</th>
              <th>creada</th>
              <th>error</th>
            </tr>
          </thead>
          <tbody>
            {items.map((ejecucion) => (
              <tr
                class="clickeable"
                onClick={() => props.navegar(`/executions/${ejecucion.id}`)}
              >
                <td><span class={`insignia ${ejecucion.status}`}>{ejecucion.status}</span></td>
                <td>{ejecucion.triggerType}</td>
                <td>{new Date(ejecucion.createdAt).toLocaleString()}</td>
                <td>{ejecucion.error?.code ?? ""}</td>
              </tr>
            ))}
            {items.length === 0 && (
              <tr><td colspan={4}>sin ejecuciones con esos filtros</td></tr>
            )}
          </tbody>
        </table>
        {cursor && (
          <div class="acciones">
            <button class="secundario" onClick={cargarMas}>cargar más</button>
          </div>
        )}
      </div>
    </div>
  );
}

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

  if (error) return <p class="error-global">{error}</p>;
  if (!detalle) return <p class="ayuda">cargando…</p>;

  const estado = String(detalle.status);
  const detalleError = detalle.error as { code?: string; message?: string } | null;

  return (
    <div>
      <h1>
        <span class={`insignia ${estado}`}>{estado}</span>{" "}
        <code style="font-size: 14px;">{props.id}</code>
      </h1>
      {detalleError && (
        <p class="error-global">
          <strong>{detalleError.code}</strong> — {detalleError.message}
        </p>
      )}
      {(detalle.replays as unknown[] | undefined)?.length ? (
        <p class="ayuda">
          replays: {(detalle.replays as { id: string; status: string }[]).length}
        </p>
      ) : null}
      {detalle.triggerData !== undefined && (
        <div class="tarjeta">
          <label>entrada del trigger (enmascarada si aplica)</label>
          <pre>{JSON.stringify(detalle.triggerData, null, 2)}</pre>
        </div>
      )}
      {pasos.map((paso) => (
        <div class={`paso ${paso.status}`} key={paso.seq}>
          <div>
            <strong>
              {paso.seq}. {paso.blockName}
            </strong>{" "}
            <span class={`insignia ${paso.status}`}>{paso.status}</span>{" "}
            {paso.durationMs !== null && (
              <span class="ayuda">{paso.durationMs} ms</span>
            )}
          </div>
          {paso.error && <p class="error-campo">{paso.error.message}</p>}
          {paso.status !== "skipped" && (
            <details>
              <summary>entrada / salida (enmascaradas si aplica)</summary>
              <pre>{JSON.stringify(paso.input, null, 2)}</pre>
              <pre>{JSON.stringify(paso.output, null, 2)}</pre>
            </details>
          )}
        </div>
      ))}
      {pasos.length === 0 && <p class="ayuda">la ejecución aún no tiene pasos</p>}
    </div>
  );
}
