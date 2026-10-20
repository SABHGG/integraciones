//! Biblioteca de bloques (MVP5, tasks 3.1-3.3): listado con filtros,
//! detalle de versiones (schemas y metadatos `ui`), creación y edición de
//! `custom_logic` con el editor de código y publicación con los errores
//! del validador en línea. Crear/editar/publicar solo para `editor`;
//! viewer inspecciona en lectura.
//! Render El rastro (MVP6): misma lógica; presentación con ui.tsx.
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
import { Boton, Campo, Control, Esqueleto, Panel } from "./ui";

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

/** Aviso de la página: verde para lo hecho, ladrillo para el error del servidor. */
function Aviso(props: { texto: string; tono?: "ok" | "mal" }) {
  const clase =
    props.tono === "mal"
      ? "border-mal/40 bg-mal/5 text-mal"
      : "border-ok/30 bg-ok-fondo text-ok";
  return (
    <div class={`rounded-[8px] border px-3.5 py-2.5 text-[13.5px] ${clase}`}>{props.texto}</div>
  );
}

/** Tipo de bloque: valor del sistema, en mono y sin color (no es un estado). */
function Etiqueta(props: { texto: string }) {
  return (
    <span class="rounded-[4px] border border-borde-fuerte bg-superficie px-2 py-1 font-mono text-[11.5px] text-tinta-suave">
      {props.texto}
    </span>
  );
}

export function BibliotecaBloques(props: { rol: Rol | null }) {
  const [lista, setLista] = useState<BloqueResumen[]>([]);
  const [texto, setTexto] = useState("");
  const [tipo, setTipo] = useState("");
  const [categoria, setCategoria] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);

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
    } finally {
      setCargando(false);
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
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h1 class="text-[21px] font-bold tracking-tight text-tinta">Biblioteca de bloques</h1>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">
          Bloques reutilizables por tus puentes: versiones, schemas y código
        </span>
      </div>

      {aviso && <Aviso texto={aviso} />}
      {error && <Aviso texto={error} tono="mal" />}

      <Panel class="overflow-hidden">
        <div class="flex flex-wrap items-center gap-3 border-b border-borde px-4 py-2.5">
          <div class="min-w-[240px] flex-1">
            <Control
              placeholder="Buscar por slug, nombre o descripción"
              aria-label="Buscar bloque"
              value={texto}
              onInput={(e: Event) => setTexto((e.target as HTMLInputElement).value)}
            />
          </div>
          <Control
            as="select"
            class="w-auto!"
            aria-label="Filtrar por tipo"
            value={tipo}
            onChange={(e: Event) => setTipo((e.target as HTMLSelectElement).value)}
          >
            <option value="">Todos los tipos</option>
            <option value="http">http</option>
            <option value="custom_logic">custom_logic</option>
          </Control>
          <Control
            as="select"
            class="w-auto!"
            aria-label="Filtrar por categoría"
            value={categoria}
            onChange={(e: Event) => setCategoria((e.target as HTMLSelectElement).value)}
          >
            <option value="">Todas las categorías</option>
            {categorias(lista).map((cat) => (
              <option key={cat} value={cat}>
                {cat}
              </option>
            ))}
          </Control>
        </div>
        <div class="overflow-x-auto">
          <table class="w-full min-w-[720px] border-collapse">
            <thead>
              <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                <th class="px-4 py-2.5 font-semibold">Slug</th>
                <th class="px-4 py-2.5 font-semibold">Nombre</th>
                <th class="px-4 py-2.5 font-semibold">Tipo</th>
                <th class="px-4 py-2.5 font-semibold">Categoría</th>
                <th class="px-4 py-2.5 font-semibold">V. vigente</th>
              </tr>
            </thead>
            <tbody>
              {visibles.map((bloque) => (
                <tr
                  key={bloque.id}
                  class="cursor-pointer border-b border-borde transition-colors last:border-0 hover:bg-tinta/3 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-tinta"
                  tabIndex={0}
                  onClick={() => void abrirDetalle(bloque)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void abrirDetalle(bloque);
                  }}
                >
                  <td class="px-4 py-2.5 font-mono text-[12.5px]">{bloque.slug}</td>
                  <td class="px-4 py-2.5 text-[13.5px]">{bloque.name}</td>
                  <td class="px-4 py-2.5">
                    <Etiqueta texto={bloque.blockType} />
                  </td>
                  <td class="px-4 py-2.5 text-[13px] text-tinta-suave">{bloque.category}</td>
                  <td class="px-4 py-2.5 font-mono text-[12.5px] text-tinta-suave">
                    {bloque.latestVersion ? `v${bloque.latestVersion}` : "—"}
                  </td>
                </tr>
              ))}
              {cargando && visibles.length === 0 && (
                <tr>
                  <td colspan={5}>
                    <Esqueleto filas={4} />
                  </td>
                </tr>
              )}
              {!cargando && visibles.length === 0 && !error && (
                <tr>
                  <td colspan={5} class="px-4 py-8 text-center text-[13.5px] text-tinta-suave">
                    {lista.length === 0
                      ? administra
                        ? "Aún no hay bloques. Crea el primero con el formulario de abajo."
                        : "Aún no hay bloques. Pide a un editor que cree el primero."
                      : "Ningún bloque coincide con esos filtros. Limpia la búsqueda o cambia el tipo."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </Panel>

      {administra && (
        <Panel class="p-4">
          <form onSubmit={crear}>
            <div class="text-[14px] font-semibold text-tinta">Nuevo bloque custom_logic</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              El slug identifica el bloque en el plano; se crea sin código y publicas la v1 abajo.
            </p>
            <div class="grid gap-x-4 gap-y-3 md:grid-cols-2">
              <Campo
                etiqueta="Slug"
                ayuda="Minúsculas, números y guiones (p. ej. enrich-order)."
                error={problemaDe("slug")}
              >
                <Control
                  id="bloque-slug"
                  mono
                  value={slug}
                  placeholder="enrich-order"
                  onInput={(e: Event) => setSlug((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Nombre" error={problemaDe("name")}>
                <Control
                  id="bloque-nombre"
                  value={nombre}
                  onInput={(e: Event) => setNombre((e.target as HTMLInputElement).value)}
                />
              </Campo>
              <Campo etiqueta="Descripción" class="md:col-span-2">
                <Control
                  id="bloque-desc"
                  value={descripcion}
                  onInput={(e: Event) => setDescripcion((e.target as HTMLInputElement).value)}
                />
              </Campo>
            </div>
            <div class="mt-3">
              <Boton type="submit">Crear bloque</Boton>
            </div>
          </form>
        </Panel>
      )}

      {detalle && (
        <Panel>
          <div class="flex flex-wrap items-center gap-3 border-b border-borde px-4 py-3">
            <span class="text-[15px] font-semibold text-tinta">{detalle.name}</span>
            <Etiqueta texto={detalle.blockType} />
            <span class="hidden text-[12.5px] text-tinta-suave lg:inline">{detalle.slug}</span>
          </div>

          {detalle.description && (
            <p class="border-b border-borde px-4 py-2.5 text-[13px] text-tinta-suave">
              {detalle.description}
            </p>
          )}

          <div class="px-4 pt-3.5 text-[14px] font-semibold text-tinta">Versiones</div>
          <div class="overflow-x-auto">
            <table class="mt-2 w-full min-w-[420px] border-collapse">
              <thead>
                <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                  <th class="px-4 py-2 font-semibold">Versión</th>
                  <th class="px-4 py-2 font-semibold">Creada</th>
                </tr>
              </thead>
              <tbody>
                {versiones.map((version) => (
                  <tr key={version.id} class="border-b border-borde last:border-0">
                    <td class="px-4 py-2 font-mono text-[12.5px]">v{version.version}</td>
                    <td class="px-4 py-2 text-[13px] text-tinta-suave">
                      {new Date(version.createdAt).toLocaleString()}
                    </td>
                  </tr>
                ))}
                {versiones.length === 0 && (
                  <tr>
                    <td colspan={2} class="px-4 py-4 text-[13.5px] text-tinta-suave">
                      {administra && detalle.blockType === "custom_logic"
                        ? "Sin versiones publicadas todavía. Escribe el código abajo y publica la v1."
                        : "Sin versiones publicadas todavía."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>

          {versiones[0] && (
            <details class="border-t border-borde px-4 py-3">
              <summary class="cursor-pointer text-[13px] text-tinta-suave hover:text-tinta">
                Input schema de la v{versiones[0].version}
              </summary>
              <pre class="mt-2 max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2.5 font-mono text-[11.5px] leading-relaxed">
                {JSON.stringify(versiones[0].inputSchema, null, 2)}
              </pre>
            </details>
          )}

          {administra && detalle.blockType === "custom_logic" && (
            <div class="border-t border-borde px-4 py-3.5">
              <div class="text-[14px] font-semibold text-tinta">Código (publicar nueva versión)</div>
              <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
                Con un error de transpile la publicación se bloquea y el validador señala la línea;
                nada se congela hasta que compila.
              </p>
              <EditorCodigo
                codigo={codigo}
                onChange={setCodigo}
                errores={erroresEditor}
              />
              {erroresEditor.length > 0 && (
                <div class="mt-2 space-y-1.5">
                  {erroresEditor.map((mensaje) => (
                    <p
                      key={mensaje}
                      class="rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 font-mono text-[12px] text-mal"
                    >
                      {mensaje}
                    </p>
                  ))}
                </div>
              )}
              <div class="mt-3">
                <Boton onClick={() => void publicarVersion(detalle)}>Publicar versión</Boton>
              </div>
            </div>
          )}
        </Panel>
      )}

      {!administra && (
        <p class="text-[12.5px] text-tinta-tenue">
          Solo un editor puede crear bloques o publicar versiones.
        </p>
      )}
    </div>
  );
}
