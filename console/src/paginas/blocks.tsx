//! Biblioteca de bloques (MVP5): listado con filtros (en la URL, para
//! conservarlos al volver del detalle), creación de `custom_logic` y — desde
//! el change add-blocks-detalle-ruta — el detalle de cada bloque es página
//! propia en `/console/blocks/{id}`. Crear/editar/publicar solo para
//! `editor`; viewer inspecciona en lectura. Render El rastro (MVP6).
import { Fragment } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { api, type Page } from "../lib/api";
import { puede, type Rol } from "../lib/roles";
import { EditorCodigo } from "../ui";
import { codigoAMostrar } from "../lib/codigo-bloque";
import {
  filtrarBloques,
  categorias,
  validarCreacionBloque,
  erroresDelPlano,
  type BloqueResumen,
  type ProblemaCampo,
} from "../lib/bloques";
import { Boton, Campo, Control, Esqueleto, Panel } from "../ui";

interface VersionBloque {
  id: string;
  blockId: string;
  version: number;
  /** JS transpilado que ejecuta el sandbox (null si la versión no tiene código). */
  code: string | null;
  /** Fuente TypeScript original del autor (null si no existe). */
  sourceCode: string | null;
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

export function BibliotecaBloques(props: { rol: Rol | null; navegar: (ruta: string) => void }) {
  const [lista, setLista] = useState<BloqueResumen[]>([]);
  const [texto, setTexto] = useState("");
  const [tipo, setTipo] = useState("");
  const [categoria, setCategoria] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);

  // Creación de custom_logic
  const [slug, setSlug] = useState("");
  const [nombre, setNombre] = useState("");
  const [descripcion, setDescripcion] = useState("");
  const [problemas, setProblemas] = useState<ProblemaCampo[]>([]);

  const administra = puede(props.rol, "administrar-recursos");

  // Filtros en la URL (?q=&tipo=&cat=): leer al montar para conservarlos al
  // volver del detalle, escribir con replaceState ante cada cambio.
  useEffect(() => {
    const query = new URLSearchParams(location.search);
    setTexto(query.get("q") ?? "");
    setTipo(query.get("tipo") ?? "");
    setCategoria(query.get("cat") ?? "");
  }, []);

  useEffect(() => {
    const query = new URLSearchParams();
    if (texto) query.set("q", texto);
    if (tipo) query.set("tipo", tipo);
    if (categoria) query.set("cat", categoria);
    const cadena = query.toString();
    history.replaceState(history.state, "", `/console/blocks${cadena ? `?${cadena}` : ""}`);
  }, [texto, tipo, categoria]);

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
      setAviso(`Bloque ${creado.slug} creado: entra y publica su v1.`);
      await cargar();
      props.navegar(`/blocks/${creado.id}`);
    } catch (e) {
      if (e instanceof Error && "fields" in e) {
        setProblemas((e as { fields: ProblemaCampo[] }).fields);
      }
      setError(e instanceof Error ? e.message : "no se pudo crear el bloque");
    }
  };

  const visibles = filtrarBloques(lista, { texto, tipo, categoria });
  const problemaDe = (campo: string) => problemas.find((p) => p.field === campo)?.message;

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h1 class="text-[21px] font-bold tracking-tight text-tinta">Biblioteca de bloques</h1>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">
          Bloques reutilizables por tus puentes: entra en cada uno para versiones, schemas y código
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
                  onClick={() => props.navegar(`/blocks/${bloque.id}`)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") props.navegar(`/blocks/${bloque.id}`);
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
              El slug identifica el bloque en el plano; se crea sin código y publicas la v1 en su página.
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

      {!administra && (
        <p class="text-[12.5px] text-tinta-tenue">
          Solo un editor puede crear bloques o publicar versiones.
        </p>
      )}
    </div>
  );
}

const PLANTILLA_BLOQUE = "export default (ctx) => {\n  return { ok: true };\n};\n";

export function DetalleBloque(props: { id: string; rol: Rol | null; navegar: (ruta: string) => void }) {
  const [bloque, setBloque] = useState<BloqueResumen | null>(null);
  const [versiones, setVersiones] = useState<VersionBloque[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [codigo, setCodigo] = useState(PLANTILLA_BLOQUE);
  const [erroresEditor, setErroresEditor] = useState<string[]>([]);
  // Precarga del editor (design D6): solo UNA vez, en la primera carga de
  // versiones. Tras publicar, el editor conserva lo que la persona escribió en
  // vez de saltar al texto recién publicado; tampoco se precarga con `code`
  // (presentar el JS emitido como fuente editable invitaría a publicarlo como
  // si fuera el original).
  const precargado = useRef(false);
  // El editor (CodeMirror) lee `codigo` solo al montar: no se renderiza hasta
  // que la carga de versiones decidió la precarga (D6), para que monte ya con
  // el fuente vigente en vez de la plantilla.
  const [cargado, setCargado] = useState(false);

  const administra = puede(props.rol, "administrar-recursos");

  useEffect(() => {
    // Al cambiar de bloque el guard de precarga vuelve a cero.
    precargado.current = false;
    setCargado(false);
    void (async () => {
      try {
        const detalle = await api<BloqueResumen>("GET", `/blocks/${props.id}`);
        setBloque(detalle);
        const pagina = await api<Page<VersionBloque>>("GET", `/blocks/${props.id}/versions`);
        setVersiones(pagina.items);
        if (!precargado.current) {
          precargado.current = true;
          const fuente = pagina.items[0]?.sourceCode;
          if (fuente != null && fuente !== "") setCodigo(fuente);
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : "no se pudo cargar el bloque");
      } finally {
        setCargado(true);
      }
    })();
  }, [props.id]);

  const publicarVersion = async () => {
    if (!bloque) return;
    setAviso(null);
    setErroresEditor([]);
    try {
      await api("POST", `/blocks/${bloque.id}/versions`, { code: codigo });
      setAviso(`Versión publicada de ${bloque.slug}.`);
      const pagina = await api<Page<VersionBloque>>("GET", `/blocks/${bloque.id}/versions`);
      setVersiones(pagina.items);
    } catch (e) {
      if (e instanceof Error) {
        const apiError = e as { status?: number; message: string; fields?: ProblemaCampo[] };
        setErroresEditor(erroresDelPlano(apiError.message, apiError.fields ?? []));
      }
    }
  };

  // Volver: si llegamos navegando dentro de la consola, atrás restaura la
  // lista con sus filtros; si la URL fue directa, a la biblioteca plana.
  const volver = () => {
    if ((history.state as { app?: boolean } | null)?.app) {
      history.back();
    } else {
      props.navegar("/blocks");
    }
  };

  if (error) {
    return (
      <div class="space-y-3">
        <Aviso texto={error} tono="mal" />
        <Boton variante="secundario" chico onClick={() => props.navegar("/blocks")}>
          Volver a la biblioteca
        </Boton>
      </div>
    );
  }

  if (!bloque) return <Panel><Esqueleto filas={4} /></Panel>;

  return (
    <div class="space-y-3">
      <div>
        <Boton variante="secundario" chico onClick={volver}>← Volver</Boton>
      </div>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
        <h1 class="font-mono text-[19px] font-semibold tracking-tight text-tinta">{bloque.slug}</h1>
        <Etiqueta texto={bloque.blockType} />
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">{bloque.name}</span>
        <span class="font-mono text-[12.5px] text-tinta-suave">
          {bloque.latestVersion ? `v${bloque.latestVersion} vigente` : "sin publicar"}
        </span>
      </div>

      {aviso && <Aviso texto={aviso} />}
      {bloque.description && (
        <p class="text-[13px] text-tinta-suave">{bloque.description}</p>
      )}

      <Panel class="overflow-hidden">
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
              {versiones.map((version) => {
                const { codigo: visible, etiqueta } = codigoAMostrar(version);
                return (
                  <Fragment key={version.id}>
                    <tr class="border-b border-borde last:border-0">
                      <td class="px-4 py-2 font-mono text-[12.5px]">v{version.version}</td>
                      <td class="px-4 py-2 text-[13px] text-tinta-suave">
                        {new Date(version.createdAt).toLocaleString()}
                      </td>
                    </tr>
                    {visible !== null ? (
                      <tr class="border-b border-borde last:border-0">
                        <td colspan={2} class="px-4 pb-3 pt-0">
                          <details>
                            <summary class="cursor-pointer text-[13px] text-tinta-suave hover:text-tinta">
                              Código de la v{version.version}
                              {etiqueta && (
                                <span class="ml-2 text-[12px] text-tinta-tenue">{etiqueta}</span>
                              )}
                            </summary>
                            <pre class="mt-2 max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2.5 font-mono text-[11.5px] leading-relaxed">
                              {visible}
                            </pre>
                          </details>
                        </td>
                      </tr>
                    ) : (
                      <tr class="border-b border-borde last:border-0">
                        <td colspan={2} class="px-4 pb-3 pt-0 text-[12.5px] text-tinta-tenue">
                          v{version.version}: {etiqueta}
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
              {versiones.length === 0 && (
                <tr>
                  <td colspan={2} class="px-4 py-4 text-[13.5px] text-tinta-suave">
                    {administra && bloque.blockType === "custom_logic"
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

        {administra && bloque.blockType === "custom_logic" && cargado && (
          <div class="border-t border-borde px-4 py-3.5">
            <div class="text-[14px] font-semibold text-tinta">Código (publicar nueva versión)</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              Con un error de transpile la publicación se bloquea y el validador señala la línea;
              nada se congela hasta que compila.
            </p>
            <EditorCodigo codigo={codigo} onChange={setCodigo} errores={[]} />
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
              <Boton onClick={() => void publicarVersion()}>Publicar versión</Boton>
            </div>
          </div>
        )}
      </Panel>
    </div>
  );
}
