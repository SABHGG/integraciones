//! Gestión de puentes: lista + editor con canvas de la cadena (orden
//! lineal, when, switch), formularios por metadatos ui, validación del
//! servidor pintada sobre cada instancia, dry-run y publicación.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { mapearErrores, type Issue } from "./errors";
import { FormularioInstancia } from "./blockform";
import type { Paso } from "./executions";
import { EditorCodigo } from "./codeeditor";
import { puede, type Rol } from "./roles";

interface Puente {
  id: string;
  slug: string;
  name: string;
  latestVersion: number | null;
  draft: Record<string, unknown> | null;
}

interface Instancia {
  id: string;
  name: string;
  blockId: string;
  blockVersionId: string;
  input: Record<string, unknown>;
  config: Record<string, unknown>;
  when?: unknown;
  code?: string | null;
}

interface VersionBloque {
  id: string;
  version: number;
  ui: Record<string, unknown> | null;
  inputSchema: Record<string, unknown>;
}

interface Bloque {
  id: string;
  slug: string;
  name: string;
  blockType: string;
}

export function ListaPuentes(props: { navegar: (ruta: string) => void }) {
  const [puentes, setPuentes] = useState<Puente[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api<Page<Puente>>("GET", "/bridges?limit=100")
      .then((pagina) => setPuentes(pagina.items))
      .catch((e) => setError(e instanceof Error ? e.message : "error al listar"));
  }, []);

  return (
    <div>
      <h1>Puentes</h1>
      {error && <p class="error-global">{error}</p>}
      <div class="tarjeta">
        <table>
          <thead>
            <tr><th>slug</th><th>nombre</th><th>versión publicada</th></tr>
          </thead>
          <tbody>
            {puentes.map((puente) => (
              <tr class="clickeable" key={puente.id} onClick={() => props.navegar(`/bridges/${puente.id}`)}>
                <td>{puente.slug}</td>
                <td>{puente.name}</td>
                <td>{puente.latestVersion ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function EditorPuente(props: { id: string; rol: Rol | null }) {
  const [puente, setPuente] = useState<Puente | null>(null);
  const [draft, setDraft] = useState<Record<string, unknown>>({});
  const [bloques, setBloques] = useState<Bloque[]>([]);
  const [versiones, setVersiones] = useState<Map<string, VersionBloque>>(new Map());
  const [issues, setIssues] = useState<Issue[]>([]);
  const [aviso, setAviso] = useState<string | null>(null);
  const [datosDryRun, setDatosDryRun] = useState("{}");
  const [dryRun, setDryRun] = useState<Record<string, unknown> | null>(null);

  useEffect(() => {
    void (async () => {
      const puente = await api<Puente>("GET", `/bridges/${props.id}`);
      setPuente(puente);
      setDraft((puente.draft ?? {}) as Record<string, unknown>);
      const pagina = await api<Page<Bloque>>("GET", "/blocks?limit=200");
      setBloques(pagina.items);
      // ui de la última versión de cada bloque (para los formularios).
      const mapa = new Map<string, VersionBloque>();
      for (const bloque of pagina.items) {
        const detalle = await api<{ versions: VersionBloque[] }>("GET", `/blocks/${bloque.id}`);
        const ultima = detalle.versions.at(-1);
        if (ultima) mapa.set(bloque.id, ultima);
      }
      setVersiones(mapa);
    })();
  }, [props.id]);

  if (!puente) return <p class="ayuda">cargando…</p>;
  const administra = puede(props.rol, "administrar-recursos");
  const ejecuta = puede(props.rol, "ejecutar");

  const instancias = (draft.blocks as Instancia[] | undefined) ?? [];

  const actualizar = (cambios: Partial<Record<string, unknown>>) => {
    setDraft((anterior) => ({ ...anterior, ...cambios }));
    setIssues([]);
  };

  const moverInstancia = (indice: number, delta: number) => {
    const nuevas = [...instancias];
    const destino = indice + delta;
    if (destino < 0 || destino >= nuevas.length) return;
    [nuevas[indice], nuevas[destino]] = [nuevas[destino], nuevas[indice]];
    actualizar({ blocks: nuevas });
  };

  const anadirInstancia = (bloque: Bloque) => {
    const version = versiones.get(bloque.id);
    const instancia: Instancia = {
      id: `b${Date.now().toString(36)}`,
      name: bloque.slug,
      blockId: bloque.id,
      blockVersionId: version?.id ?? "",
      input: {},
      config: {},
    };
    actualizar({ blocks: [...instancias, instancia] });
  };

  const guardar = async () => {
    setAviso(null);
    try {
      // Las secciones van sueltas en el PATCH (contrato del plano admin).
      const { trigger, inputSchema, blocks, mask, emit, webhook } = draft;
      await api("PATCH", `/bridges/${puente.id}`, {
        trigger,
        inputSchema: inputSchema ?? {},
        blocks,
        ...(mask !== undefined ? { mask } : {}),
        ...(emit !== undefined ? { emit } : {}),
        ...(webhook !== undefined ? { webhook } : {}),
      });
      setAviso("borrador guardado");
    } catch (e) {
      setAviso(e instanceof Error ? e.message : "error al guardar");
    }
  };

  const validar = async () => {
    setAviso(null);
    try {
      const resultado = await api<{ valid: boolean; errors: Issue[] }>(
        "POST",
        `/bridges/${puente.id}/validate`,
      );
      setIssues(resultado.errors);
      setAviso(resultado.valid ? "el borrador es válido" : "hay errores de validación");
    } catch (e) {
      setAviso(e instanceof Error ? e.message : "error al validar");
    }
  };

  const publicar = async () => {
    setAviso(null);
    try {
      await api("POST", `/bridges/${puente.id}/versions`);
      setAviso("versión publicada");
      const actualizado = await api<Puente>("GET", `/bridges/${props.id}`);
      setPuente(actualizado);
    } catch (e) {
      // Un 422 bloquea la publicación: los issues se pintan sobre el canvas.
      if (e && typeof e === "object" && "fields" in e) {
        setIssues((e as { fields: Issue[] }).fields);
      }
      setAviso(e instanceof Error ? e.message : "error al publicar");
    }
  };

  const ensayar = async () => {
    setAviso(null);
    setDryRun(null);
    try {
      let datos: unknown = {};
      try {
        datos = JSON.parse(datosDryRun);
      } catch {
        setAviso("los datos de prueba no son JSON válido");
        return;
      }
      const resultado = await api<Record<string, unknown>>(
        "POST",
        `/bridges/${puente.id}/dry-run`,
        { triggerData: datos },
      );
      setDryRun(resultado);
    } catch (e) {
      setAviso(e instanceof Error ? e.message : "error en el dry-run");
    }
  };

  const mapeado = mapearErrores(issues);
  const trigger = (draft.trigger ?? {}) as Record<string, unknown>;

  return (
    <div>
      <h1>
        {puente.slug}{" "}
        <span class="insignia">{puente.latestVersion ? `v${puente.latestVersion}` : "sin publicar"}</span>
      </h1>
      {aviso && <p class="ayuda">{aviso}</p>}
      {mapeado.globales.map((issue) => (
        <p class="error-global">
          <strong>{issue.field}</strong> — {issue.message}
        </p>
      ))}

      <div class="tarjeta">
        <h3>trigger</h3>
        <FormularioInstancia
          ui={null}
          config={trigger}
          onChange={(config) => actualizar({ trigger: config })}
          errores={mapeado.globales.filter((i) => i.field.startsWith("trigger"))}
        />
      </div>

      <h2>cadena</h2>
      {instancias.map((instancia, indice) => {
        const version = versiones.get(instancia.blockId);
        const errores = mapeado.porInstancia.get(indice) ?? [];
        return (
          <div key={instancia.id}>
            <div class={`instancia ${errores.length ? "invalida" : ""}`}>
              <div class="cabecera">
                <span class="nombre">{indice + 1}. {instancia.name}</span>
                {instancia.when !== undefined && <span class="insignia">when</span>}
              </div>
              {instancia.code !== null && instancia.code !== undefined ? (
                <EditorCodigo
                  codigo={String(instancia.code)}
                  onChange={(codigo) => {
                    const nuevas = [...instancias];
                    nuevas[indice] = { ...instancia, code: codigo };
                    actualizar({ blocks: nuevas });
                  }}
                  errores={errores.map((issue) => issue.message)}
                />
              ) : (
                <FormularioInstancia
                  ui={(version?.ui ?? null) as never}
                  config={instancia.config}
                  onChange={(config) => {
                    const nuevas = [...instancias];
                    nuevas[indice] = { ...instancia, config };
                    actualizar({ blocks: nuevas });
                  }}
                  errores={errores}
                />
              )}
              <label>when (condición declarativa, JSON o vacío)</label>
              <textarea
                rows={2}
                value={instancia.when === undefined ? "" : JSON.stringify(instancia.when)}
                onInput={(e) => {
                  const crudo = (e.target as HTMLTextAreaElement).value.trim();
                  const nuevas = [...instancias];
                  if (crudo === "") {
                    delete nuevas[indice].when;
                  } else {
                    try {
                      nuevas[indice] = { ...instancia, when: JSON.parse(crudo) };
                    } catch {
                      // JSON incompleto mientras se escribe.
                    }
                  }
                  actualizar({ blocks: nuevas });
                }}
              />
              {errores
                .filter((issue) => issue.field.endsWith(".when"))
                .map((issue) => (
                  <p class="error-campo">{issue.message}</p>
                ))}
              {administra && (
                <div class="acciones">
                  <button class="secundario" onClick={() => moverInstancia(indice, -1)} disabled={indice === 0}>↑</button>
                  <button class="secundario" onClick={() => moverInstancia(indice, 1)} disabled={indice === instancias.length - 1}>↓</button>
                  <button
                    class="peligro"
                    onClick={() => actualizar({ blocks: instancias.filter((_, i) => i !== indice) })}
                  >
                    quitar
                  </button>
                </div>
              )}
            </div>
            {indice < instancias.length - 1 && <p class="flecha">↓</p>}
          </div>
        );
      })}

      {administra && (
      <div class="tarjeta">
        <h3>añadir instancia</h3>
          <div class="acciones">
            {bloques.map((bloque) => (
              <button class="secundario" key={bloque.id} onClick={() => anadirInstancia(bloque)}>
                + {bloque.slug}
              </button>
            ))}
          </div>
        </div>
      )}

      {ejecuta && (
      <div class="tarjeta">
        <h3>dry-run (sin efectos)</h3>
        <label>datos de prueba para $.trigger (JSON)</label>
        <textarea value={datosDryRun} onInput={(e) => setDatosDryRun((e.target as HTMLTextAreaElement).value)} rows={3} />
            <div class="acciones">
              <button class="secundario" onClick={ensayar}>ensayar</button>
            </div>
        {dryRun && (
          <div>
            <p>
              <span class={`insignia ${dryRun.status}`}>{String(dryRun.status)}</span>
            </p>
            {((dryRun.steps as Paso[]) ?? []).map((paso) => (
              <div class={`paso ${paso.status}`} key={paso.seq}>
                <strong>{paso.seq}. {paso.blockName}</strong>{" "}
                <span class={`insignia ${paso.status}`}>{paso.status}</span>
                {paso.error && <p class="error-campo">{paso.error.message}</p>}
                <details>
                  <summary>entrada / salida</summary>
                  <pre>{JSON.stringify(paso.input, null, 2)}</pre>
                  <pre>{JSON.stringify(paso.output, null, 2)}</pre>
                </details>
              </div>
            ))}
            <details>
              <summary>salida final</summary>
              <pre>{JSON.stringify(dryRun.output, null, 2)}</pre>
            </details>
          </div>
        )}
      </div>
      )}

      {administra && (
      <div class="acciones" style="margin: 16px 0;">
        <button onClick={guardar}>guardar borrador</button>
        <button class="secundario" onClick={validar}>validar</button>
        <button class="secundario" onClick={publicar}>publicar versión</button>
      </div>
      )}
    </div>
  );
}
