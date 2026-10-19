//! Biblioteca de bloques (MVP5, tasks 3.1-3.3): listado con filtros,
//! detalle de versiones (schemas y metadatos `ui`), creación y edición de
//! `custom_logic` con el editor de código y publicación con los errores
//! del validador en línea. Crear/editar/publicar solo para `editor`;
//! viewer inspecciona en lectura.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "./api";
import { puede, type Rol } from "./roles";
import { EditorCodigo } from "./codeeditor";
import {
  filtrarBloques,
  categorias,
  validarCreacionBloque,
  erroresDelPlano,
  type BloqueResumen,
  type ProblemaCampo,
} from "./bloques";

interface VersionBloque {
  id: string;
  blockId: string;
  version: number;
  code: string | null;
  configSchema: unknown;
  inputSchema: unknown;
  outputSchema: unknown;
  ui: unknown;
  createdAt: string;
}

export function BibliotecaBloques(props: { rol: Rol | null }) {
  const [lista, setLista] = useState<BloqueResumen[]>([]);
  const [texto, setTexto] = useState("");
  const [tipo, setTipo] = useState("");
  const [categoria, setCategoria] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);

  // Detalle seleccionado
  const [detalle, setDetalle] = useState<BloqueResumen | null>(null);
  const [versiones, setVersiones] = useState<VersionBloque[]>([]);

  // Creación de custom_logic + publicación de la primera versión
  const [slug, setSlug] = useState("");
  const [nombre, setNombre] = useState("");
  const [descripcion, setDescripcion] = useState("");
  const [problemas, setProblemas] = useState<ProblemaCampo[]>([]);
  const [codigo, setCodigo] = useState(
    "export default (ctx) => {\n  return { ok: true };\n};\n",
  );
  const [erroresEditor, setErroresEditor] = useState<string[]>([]);

  const administra = puede(props.rol, "administrar-recursos");

  const cargar = async () => {
    setError(null);
    try {
      const pagina = await api<Page<BloqueResumen>>("GET", "/blocks");
      setLista(pagina.items);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error al listar bloques");
    }
  };

  useEffect(() => {
    void cargar();
  }, []);

  const abrirDetalle = async (bloque: BloqueResumen) => {
    setError(null);
    setDetalle(bloque);
    setErroresEditor([]);
    try {
      const pagina = await api<Page<VersionBloque>>(
        "GET",
        `/blocks/${bloque.id}/versions`,
      );
      setVersiones(pagina.items);
    } catch (e) {
      setVersiones([]);
      setError(e instanceof Error ? e.message : "no se pudieron listar las versiones");
    }
  };

  const crear = async (evento: Event) => {
    evento.preventDefault();
    setAviso(null);
    const issues = validarCreacionBloque({ slug, name: nombre });
    setProblemas(issues);
    if (issues.length > 0) return;
    try {
      const creado = await api<BloqueResumen>("POST", "/blocks", {
        slug: slug.trim(),
        name: nombre.trim(),
        blockType: "custom_logic",
        category: "dominio",
        description: descripcion.trim(),
      });
      setSlug("");
      setNombre("");
      setDescripcion("");
      setAviso(`bloque ${creado.slug} creado: escribe su código y publica la v1`);
      await cargar();
      void abrirDetalle(creado);
    } catch (e) {
      if (e instanceof Error && "fields" in e) {
        setProblemas((e as { fields: ProblemaCampo[] }).fields);
      }
      setError(e instanceof Error ? e.message : "no se pudo crear el bloque");
    }
  };

  const publicarVersion = async (bloque: BloqueResumen) => {
    setAviso(null);
    setError(null);
    setErroresEditor([]);
    try {
      await api("POST", `/blocks/${bloque.id}/versions`, { code: codigo });
      setAviso(`versión publicada de ${bloque.slug}`);
      await cargar();
      void abrirDetalle(bloque);
    } catch (e) {
      if (e instanceof Error) {
        const apiError = e as { status?: number; message: string; fields?: ProblemaCampo[] };
        setErroresEditor(erroresDelPlano(apiError.message, apiError.fields ?? []));
      }
    }
  };

  const visibles = filtrarBloques(lista, { texto, tipo, categoria });
  const problemaDe = (campo: string) => problemas.find((p) => p.field === campo)?.message;

  return (
    <div>
      <h1>Biblioteca de bloques</h1>
      {aviso && <p class="ayuda">{aviso}</p>}
      {error && <p class="error-global">{error}</p>}

      <div class="tarjeta">
        <div class="acciones" style="margin-bottom: 8px;">
          <input
            placeholder="buscar por slug, nombre o descripción"
            style="flex: 1;"
            value={texto}
            onInput={(e) => setTexto((e.target as HTMLInputElement).value)}
          />
          <select value={tipo} onChange={(e) => setTipo((e.target as HTMLSelectElement).value)}>
            <option value="">todos los tipos</option>
            <option value="http">http</option>
            <option value="custom_logic">custom_logic</option>
          </select>
          <select
            value={categoria}
            onChange={(e) => setCategoria((e.target as HTMLSelectElement).value)}
          >
            <option value="">todas las categorías</option>
            {categorias(lista).map((cat) => (
              <option key={cat} value={cat}>
                {cat}
              </option>
            ))}
          </select>
        </div>
        <table>
          <thead>
            <tr><th>slug</th><th>nombre</th><th>tipo</th><th>categoría</th><th>v. vigente</th></tr>
          </thead>
          <tbody>
            {visibles.map((bloque) => (
              <tr
                key={bloque.id}
                style="cursor: pointer;"
                onClick={() => void abrirDetalle(bloque)}
              >
                <td>{bloque.slug}</td>
                <td>{bloque.name}</td>
                <td><span class="insignia">{bloque.blockType}</span></td>
                <td>{bloque.category}</td>
                <td>{bloque.latestVersion ?? "—"}</td>
              </tr>
            ))}
            {visibles.length === 0 && <tr><td colspan={5}>sin bloques que coincidan</td></tr>}
          </tbody>
        </table>
      </div>

      {administra && (
        <form class="tarjeta" onSubmit={crear}>
          <h3>nuevo bloque custom_logic</h3>
          <label for="bloque-slug">slug</label>
          <input id="bloque-slug" value={slug}
            onInput={(e) => setSlug((e.target as HTMLInputElement).value)}
            placeholder="enrich-order" />
          {problemaDe("slug") && <p class="error-campo">{problemaDe("slug")}</p>}
          <label for="bloque-nombre">nombre</label>
          <input id="bloque-nombre" value={nombre}
            onInput={(e) => setNombre((e.target as HTMLInputElement).value)} />
          {problemaDe("name") && <p class="error-campo">{problemaDe("name")}</p>}
          <label for="bloque-desc">descripción</label>
          <input id="bloque-desc" value={descripcion}
            onInput={(e) => setDescripcion((e.target as HTMLInputElement).value)} />
          <div class="acciones">
            <button type="submit">crear bloque</button>
          </div>
        </form>
      )}

      {detalle && (
        <div class="tarjeta">
          <h3>
            {detalle.name} <span class="insignia">{detalle.blockType}</span>
          </h3>
          <p class="ayuda">{detalle.description}</p>

          <h4>versiones</h4>
          <table>
            <thead>
              <tr><th>versión</th><th>creada</th></tr>
            </thead>
            <tbody>
              {versiones.map((version) => (
                <tr key={version.id}>
                  <td>v{version.version}</td>
                  <td>{new Date(version.createdAt).toLocaleString()}</td>
                </tr>
              ))}
              {versiones.length === 0 && (
                <tr><td colspan={2}>sin versiones publicadas</td></tr>
              )}
            </tbody>
          </table>

          {versiones[0] && (
            <>
              <h4>input schema de la v{versiones[0].version}</h4>
              <details>
                <summary>input schema</summary>
                <pre>{JSON.stringify(versiones[0].inputSchema, null, 2)}</pre>
              </details>
            </>
          )}

          {administra && detalle.blockType === "custom_logic" && (
            <>
              <h4>código (publicar nueva versión)</h4>
              <EditorCodigo
                codigo={codigo}
                onChange={setCodigo}
                errores={erroresEditor}
              />
              {erroresEditor.length > 0 && (
                <div class="error-global">
                  {erroresEditor.map((mensaje) => (
                    <p key={mensaje} class="error-campo">{mensaje}</p>
                  ))}
                </div>
              )}
              <div class="acciones">
                <button onClick={() => void publicarVersion(detalle)}>
                  publicar versión
                </button>
              </div>
              <p class="ayuda">
                Con un error de transpile la publicación se bloquea y el
                validador señala la línea; nada se congela hasta que compila.
              </p>
            </>
          )}
        </div>
      )}

      {!administra && (
        <p class="ayuda">solo un editor puede crear bloques o publicar versiones</p>
      )}
    </div>
  );
}
