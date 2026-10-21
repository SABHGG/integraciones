//! Gestión de puentes: lista + editor con canvas de la cadena (orden
//! lineal, when, switch), formularios por metadatos ui, validación del
//! servidor pintada sobre cada instancia, dry-run y publicación.
//! Render El rastro (MVP6): la cadena se presenta como los pasos del rastro
//! (nudo + raíl); lógica intacta desde el MVP3/MVP5.
import { useEffect, useState } from "preact/hooks";
import { api, type Page } from "../lib/api";
import { mapearErrores, type Issue } from "../lib/errors";
import { FormularioInstancia } from "../ui";
import type { Paso } from "./executions";
import { EditorCodigo } from "../ui";
import { puede, type Rol } from "../lib/roles";
import { PanelVersiones } from "./versiones";
import {
  artefactoExport,
  validarImport,
  sufijarSlug,
  type ArtefactoPuente,
} from "../lib/transferir";
import { Boton, Campo, Control, Esqueleto, Panel } from "../ui";

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

function Aviso(props: { texto: string; tono?: "ok" | "mal" }) {
  const clase =
    props.tono === "mal"
      ? "border-mal/40 bg-mal/5 text-mal"
      : "border-ok/30 bg-ok-fondo text-ok";
  return (
    <div class={`rounded-[8px] border px-3.5 py-2.5 text-[13.5px] ${clase}`}>{props.texto}</div>
  );
}

export function ListaPuentes(props: { navegar: (ruta: string) => void; rol: Rol | null }) {
  const [puentes, setPuentes] = useState<Puente[]>([]);
  const [cargando, setCargando] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [aviso, setAviso] = useState<string | null>(null);
  const [textoImport, setTextoImport] = useState("");
  const [publicarAlImportar, setPublicarAlImportar] = useState(false);
  const [erroresValidacion, setErroresValidacion] = useState<string[]>([]);
  const [nombreNuevo, setNombreNuevo] = useState("");
  const [slugNuevo, setSlugNuevo] = useState("");
  const [slugEditado, setSlugEditado] = useState(false);
  const [erroresNuevo, setErroresNuevo] = useState<string[]>([]);
  const [creando, setCreando] = useState(false);

  const refrescar = () =>
    api<Page<Puente>>("GET", "/bridges?limit=100").then((pagina) => setPuentes(pagina.items));

  useEffect(() => {
    void refrescar()
      .catch((e) => setError(e instanceof Error ? e.message : "error al listar"))
      .finally(() => setCargando(false));
  }, []);

  const administra = puede(props.rol, "administrar-recursos");

  /// Slug sugerido desde el nombre: minúsculas, sin acentos, guiones simples
  /// (el patrón lo valida el plano; aquí solo sugerimos).
  const sugerirSlug = (nombre: string) =>
    nombre
      .toLowerCase()
      .normalize("NFD")
      .replace(/[\u0300-\u036f]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");

  // Crear desde cero (spec web-console «Vacío que invita» + taller): POST del
  // borrador vacío y directo a su editor; el disparador, la cadena y la
  // publicación se hacen ahí.
  const crear = async (evento: Event) => {
    evento.preventDefault();
    setAviso(null);
    setError(null);
    setErroresNuevo([]);
    const slug = (slugNuevo.trim() || sugerirSlug(nombreNuevo)).trim();
    if (!nombreNuevo.trim() || !slug) {
      setErroresNuevo(["nombre: el puente necesita un nombre (y un slug)"]);
      return;
    }
    setCreando(true);
    try {
      const creado = await api<Puente>("POST", "/bridges", { slug, name: nombreNuevo.trim() });
      props.navegar(`/bridges/${creado.id}`);
    } catch (e) {
      if (e instanceof Error && "fields" in e) {
        const campos = (e as { fields: { field: string; message: string }[] }).fields;
        setErroresNuevo(campos.map((f) => `${f.field}: ${f.message}`));
      }
      setError(e instanceof Error ? e.message : "no se pudo crear el puente");
    } finally {
      setCreando(false);
    }
  };

  const descargar = (artefacto: ArtefactoPuente) => {
    const blob = new Blob([artefactoExport(artefacto)], { type: "application/json" });
    const enlace = document.createElement("a");
    enlace.href = URL.createObjectURL(blob);
    enlace.download = `${artefacto.slug}.puente.json`;
    enlace.click();
    URL.revokeObjectURL(enlace.href);
  };

  // Export del borrador (lectura: cualquier rol).
  const exportarBorrador = async (puente: Puente) => {
    setError(null);
    try {
      const detalle = await api<Puente>("GET", `/bridges/${puente.id}`);
      descargar({
        name: detalle.name,
        slug: detalle.slug,
        definition: detalle.draft ?? {},
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo exportar");
    }
  };

  // Duplicar: export interno → nuevo puente con slug sufijado (editor).
  const duplicar = async (puente: Puente) => {
    setAviso(null);
    setError(null);
    try {
      const detalle = await api<Puente>("GET", `/bridges/${puente.id}`);
      const nuevo = await api<Puente>("POST", "/bridges", {
        slug: sufijarSlug(detalle.slug),
        name: `${detalle.name} (copia)`,
      });
      const secciones = ["trigger", "inputSchema", "blocks", "mask", "emit", "webhook"];
      const cuerpo: Record<string, unknown> = {};
      const definicion = (detalle.draft ?? {}) as Record<string, unknown>;
      for (const clave of secciones) {
        if (clave in definicion) cuerpo[clave] = definicion[clave];
      }
      await api("PATCH", `/bridges/${nuevo.id}`, cuerpo);
      setAviso(`Duplicado como ${nuevo.slug} (borrador listo; publica cuando quieras).`);
      void refrescar();
    } catch (e) {
      setError(e instanceof Error ? e.message : "no se pudo duplicar");
    }
  };

  // Import (task 6.2 del MVP5): crear → PATCH del borrador → validate del
  // plano → publicación SOLO si el usuario la marcó y la validación pasa.
  const importar = async (evento: Event) => {
    evento.preventDefault();
    setAviso(null);
    setError(null);
    setErroresValidacion([]);
    const resultado = validarImport(textoImport);
    if (!resultado.ok) {
      setError(resultado.error);
      return;
    }
    const { datos } = resultado;
    try {
      const creado = await api<Puente>("POST", "/bridges", {
        slug: datos.slug,
        name: datos.name,
      });
      const secciones = ["trigger", "inputSchema", "blocks", "mask", "emit", "webhook"];
      const cuerpo: Record<string, unknown> = {};
      const definicion = datos.definition as Record<string, unknown>;
      for (const clave of secciones) {
        if (clave in definicion) cuerpo[clave] = definicion[clave];
      }
      await api("PATCH", `/bridges/${creado.id}`, cuerpo);
      await api("POST", `/bridges/${creado.id}/validate`);
      if (publicarAlImportar) {
        await api("POST", `/bridges/${creado.id}/versions`);
        setAviso(`Importado y publicado como ${creado.slug}.`);
      } else {
        setAviso(`Importado como ${creado.slug} (borrador validado; publica en su editor).`);
      }
      setTextoImport("");
      void refrescar();
    } catch (e) {
      if (e instanceof Error && "fields" in e) {
        const campos = (e as { fields: { field: string; message: string }[] }).fields;
        setErroresValidacion(campos.map((f) => `${f.field}: ${f.message}`));
      }
      setError(e instanceof Error ? e.message : "no se pudo importar");
    }
  };

  return (
    <div class="space-y-3">
      <div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
        <h1 class="text-[21px] font-bold tracking-tight text-tinta">Puentes</h1>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">
          Cadenas de bloques disparables por HTTP, cron y eventos
        </span>
      </div>

      {aviso && <Aviso texto={aviso} />}
      {error && <Aviso texto={error} tono="mal" />}

      {administra && (
        <Panel class="p-4">
          <form onSubmit={crear}>
            <div class="text-[14px] font-semibold text-tinta">Nuevo puente</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              Crea el borrador vacío y abre su editor: disparador, cadena y publicación se hacen ahí.
            </p>
            <div class="flex flex-wrap items-end gap-3">
              <label class="flex min-w-[220px] flex-1 flex-col gap-1 text-[12.5px] text-tinta-suave">
                Nombre
                <Control
                  value={nombreNuevo}
                  placeholder="Personajes de Rick and Morty"
                  onInput={(e: Event) => {
                    const valor = (e.target as HTMLInputElement).value;
                    setNombreNuevo(valor);
                    if (!slugEditado) setSlugNuevo(sugerirSlug(valor));
                  }}
                />
              </label>
              <label class="flex min-w-[180px] flex-col gap-1 text-[12.5px] text-tinta-suave">
                Slug
                <Control
                  mono
                  value={slugNuevo}
                  placeholder="personajes-rm"
                  onInput={(e: Event) => {
                    setSlugEditado(true);
                    setSlugNuevo((e.target as HTMLInputElement).value);
                  }}
                />
              </label>
              <Boton type="submit" procesando={creando} deshabilitado={creando}>
                Crear y abrir el editor
              </Boton>
            </div>
            {erroresNuevo.length > 0 && (
              <div class="mt-2 space-y-1.5">
                {erroresNuevo.map((mensaje) => (
                  <p key={mensaje} class="rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 font-mono text-[12px] text-mal">
                    {mensaje}
                  </p>
                ))}
              </div>
            )}
          </form>
        </Panel>
      )}

      {administra && (
        <Panel class="p-4">
          <form onSubmit={importar}>
            <div class="text-[14px] font-semibold text-tinta">Importar puente</div>
            <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
              Pega un artefacto exportado {"{name, slug, definition}"}:
            </p>
            <Control
              as="textarea"
              mono
              rows={6}
              placeholder='{"name": "…", "slug": "…", "definition": {…}}'
              value={textoImport}
              onInput={(e: Event) => setTextoImport((e.target as HTMLTextAreaElement).value)}
            />
            {erroresValidacion.length > 0 && (
              <div class="mt-2 space-y-1.5">
                {erroresValidacion.map((mensaje) => (
                  <p key={mensaje} class="rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 font-mono text-[12px] text-mal">
                    {mensaje}
                  </p>
                ))}
              </div>
            )}
            <div class="mt-3 flex flex-wrap items-center gap-3">
              <label class="flex items-center gap-1.5 text-[12.5px] text-tinta-suave">
                <input
                  type="checkbox"
                  class="h-4 w-4 accent-[#14352d]"
                  checked={publicarAlImportar}
                  onChange={(e) => setPublicarAlImportar((e.target as HTMLInputElement).checked)}
                />
                Publicar versión tras validar
              </label>
              <span class="text-[12px] text-tinta-tenue">Nunca publica un borrador inválido.</span>
              <div class="ml-auto">
                <Boton type="submit" deshabilitado={textoImport.trim() === ""}>Importar</Boton>
              </div>
            </div>
          </form>
        </Panel>
      )}

      <Panel class="overflow-hidden">
        <div class="overflow-x-auto">
          <table class="w-full min-w-[640px] border-collapse">
            <thead>
              <tr class="border-b border-borde text-left text-[12px] font-semibold text-tinta-suave">
                <th class="px-4 py-2.5 font-semibold">Puente</th>
                <th class="px-4 py-2.5 font-semibold">Nombre</th>
                <th class="px-4 py-2.5 font-semibold">Versión publicada</th>
                <th class="px-4 py-2.5 font-semibold">Exportar / duplicar</th>
              </tr>
            </thead>
            <tbody>
              {puentes.map((puente) => (
                <tr
                  key={puente.id}
                  class="cursor-pointer border-b border-borde transition-colors last:border-0 hover:bg-tinta/3 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-tinta"
                  tabIndex={0}
                  onClick={() => props.navegar(`/bridges/${puente.id}`)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") props.navegar(`/bridges/${puente.id}`);
                  }}
                >
                  <td class="px-4 py-2.5 font-mono text-[12.5px]">{puente.slug}</td>
                  <td class="px-4 py-2.5 text-[13.5px]">{puente.name}</td>
                  <td class="px-4 py-2.5 font-mono text-[12.5px] text-tinta-suave">
                    {puente.latestVersion ? `v${puente.latestVersion}` : "—"}
                  </td>
                  <td class="px-4 py-2.5" onClick={(e) => e.stopPropagation()}>
                    <div class="flex gap-2">
                      <Boton variante="secundario" chico onClick={() => void exportarBorrador(puente)}>
                        Exportar
                      </Boton>
                      {administra && (
                        <Boton variante="secundario" chico onClick={() => void duplicar(puente)}>
                          Duplicar
                        </Boton>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
              {cargando && puentes.length === 0 && (
                <tr><td colspan={4}><Esqueleto filas={3} /></td></tr>
              )}
              {!cargando && puentes.length === 0 && !error && (
                <tr>
                  <td colspan={4} class="px-4 py-8 text-center text-[13.5px] text-tinta-suave">
                    {administra
                      ? "Aún no hay puentes. Crea el primero con «Nuevo puente» arriba, o importa un artefacto exportado."
                      : "Aún no hay puentes. Pide a un editor que cree o importe el primero."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {!administra && (
          <p class="border-t border-borde px-4 py-2.5 text-[12.5px] text-tinta-tenue">
            Exportar es lectura; solo un editor puede importar o duplicar.
          </p>
        )}
      </Panel>
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

  const recargar = async () => {
    const puente = await api<Puente>("GET", `/bridges/${props.id}`);
    setPuente(puente);
    setDraft((puente.draft ?? {}) as Record<string, unknown>);
  };

  useEffect(() => {
    void (async () => {
      await recargar();
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

  if (!puente) return <Panel><Esqueleto filas={4} /></Panel>;
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
      setAviso("Borrador guardado.");
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
      setAviso(resultado.valid ? "El borrador es válido." : "Hay errores de validación.");
    } catch (e) {
      setAviso(e instanceof Error ? e.message : "error al validar");
    }
  };

  const publicar = async () => {
    setAviso(null);
    try {
      await api("POST", `/bridges/${puente.id}/versions`);
      setAviso("Versión publicada.");
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
        setAviso("Los datos de prueba no son JSON válido.");
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
    <div class="space-y-3">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <h1 class="font-mono text-[19px] font-semibold tracking-tight text-tinta">{puente.slug}</h1>
        <span class="rounded-[4px] border border-borde-fuerte bg-superficie px-2 py-1 font-mono text-[11.5px] text-tinta-suave">
          {puente.latestVersion ? `v${puente.latestVersion}` : "sin publicar"}
        </span>
        <span class="hidden text-[13.5px] text-tinta-suave lg:inline">{puente.name}</span>
        {administra && (
          <div class="ml-auto flex flex-wrap items-center gap-2">
            <Boton variante="secundario" chico onClick={() => void guardar()}>Guardar borrador</Boton>
            <Boton variante="secundario" chico onClick={() => void validar()}>Validar</Boton>
            <Boton chico onClick={() => void publicar()}>Publicar versión</Boton>
          </div>
        )}
      </div>

      {aviso && (
        <Aviso
          texto={aviso}
          tono={aviso.startsWith("error") || aviso.includes("Hay errores") || aviso.includes("no son JSON") ? "mal" : "ok"}
        />
      )}
      {mapeado.globales
        .filter((issue) => !issue.field.startsWith("trigger"))
        .map((issue) => (
          <Aviso key={`${issue.field}-${issue.message}`} texto={`${issue.field}: ${issue.message}`} tono="mal" />
        ))}

      <Panel class="p-4">
        <div class="mb-3 text-[14px] font-semibold text-tinta">Trigger</div>
        <FormularioInstancia
          ui={null}
          config={trigger}
          onChange={(config) => actualizar({ trigger: config })}
          errores={mapeado.globales.filter((i) => i.field.startsWith("trigger"))}
        />
      </Panel>

      <Panel class="p-4">
        <div class="mb-3 text-[14px] font-semibold text-tinta">Cadena</div>
        {instancias.length === 0 && (
          <p class="text-[13.5px] text-tinta-suave">
            La cadena está vacía: añade el primer bloque con los botones de abajo.
          </p>
        )}
        <ol>
          {instancias.map((instancia, indice) => {
            const version = versiones.get(instancia.blockId);
            const errores = mapeado.porInstancia.get(indice) ?? [];
            return (
              <li
                key={instancia.id}
                class={`relative ml-[7px] border-l-2 pl-6 ${indice === instancias.length - 1 ? "pb-1" : "pb-5"} ${errores.length ? "border-mal" : "border-borde-fuerte"}`}
              >
                <span
                  class={`absolute -left-[9px] top-0.5 h-4 w-4 rounded-full border-2 bg-superficie ${errores.length ? "border-mal bg-mal-fondo" : "border-borde-fuerte"}`}
                  aria-hidden="true"
                />
                <div class="flex flex-wrap items-center gap-2.5">
                  <span class="font-mono text-[12.5px] font-semibold text-tinta">
                    {indice + 1}. {instancia.name}
                  </span>
                  {instancia.when !== undefined && (
                    <span class="rounded-[4px] border border-borde-fuerte bg-papel px-1.5 py-0.5 font-mono text-[10.5px] text-tinta-suave">when</span>
                  )}
                </div>
                <div class="mt-2">
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
                </div>
                <div class="mt-3">
                  <Campo etiqueta="when (condición declarativa, JSON o vacío)">
                    <Control
                      as="textarea"
                      mono
                      rows={2}
                      value={instancia.when === undefined ? "" : JSON.stringify(instancia.when)}
                      onInput={(e: Event) => {
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
                  </Campo>
                </div>
                {errores
                  .filter((issue) => issue.field.endsWith(".when"))
                  .map((issue) => (
                    <p class="mt-1.5 rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 text-[12.5px] text-mal">{issue.message}</p>
                  ))}
                {administra && (
                  <div class="mt-2.5 flex gap-2">
                    <Boton variante="secundario" chico onClick={() => moverInstancia(indice, -1)} deshabilitado={indice === 0}>↑</Boton>
                    <Boton variante="secundario" chico onClick={() => moverInstancia(indice, 1)} deshabilitado={indice === instancias.length - 1}>↓</Boton>
                    <Boton
                      variante="peligro"
                      chico
                      onClick={() => actualizar({ blocks: instancias.filter((_, i) => i !== indice) })}
                    >
                      Quitar
                    </Boton>
                  </div>
                )}
              </li>
            );
          })}
        </ol>

        {administra && (
          <div class="mt-4 border-t border-borde pt-3.5">
            <div class="mb-2 text-[13.5px] font-medium text-tinta">Añadir instancia</div>
            <div class="flex flex-wrap gap-2">
              {bloques.map((bloque) => (
                <Boton variante="secundario" chico key={bloque.id} onClick={() => anadirInstancia(bloque)}>
                  + <span class="font-mono text-[11.5px]">{bloque.slug}</span>
                </Boton>
              ))}
            </div>
          </div>
        )}
      </Panel>

      {ejecuta && (
        <Panel class="p-4">
          <div class="text-[14px] font-semibold text-tinta">Ensayo sin efectos</div>
          <p class="mt-1 mb-3 text-[12.5px] text-tinta-suave">
            Corre la cadena con datos de prueba: nada se persiste ni dispara emit/webhook.
          </p>
          <Campo etiqueta="Datos de prueba para $.trigger (JSON)">
            <Control as="textarea" mono rows={3} value={datosDryRun} onInput={(e: Event) => setDatosDryRun((e.target as HTMLTextAreaElement).value)} />
          </Campo>
          <div class="mt-3">
            <Boton variante="secundario" chico onClick={() => void ensayar()}>Ensayar</Boton>
          </div>
          {dryRun && (
            <div class="mt-3 border-t border-borde pt-3">
              <p class="mb-2">
                <span class="rounded-[4px] border border-borde-fuerte bg-papel px-2 py-1 font-mono text-[11px] text-tinta-suave">
                  resultado: {String(dryRun.status)}
                </span>
              </p>
              {((dryRun.steps as Paso[]) ?? []).map((paso) => (
                <div class="mb-2 rounded-[6px] border border-borde p-2.5" key={paso.seq}>
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-mono text-[12px] font-semibold text-tinta">{paso.seq}. {paso.blockName}</span>
                    <span class={`rounded-[4px] px-1.5 py-0.5 font-mono text-[10.5px] ${paso.status === "succeeded" ? "bg-ok-fondo text-ok" : paso.status === "failed" ? "bg-mal-fondo text-mal" : "bg-aviso-fondo text-aviso"}`}>
                      {paso.status}
                    </span>
                    {paso.durationMs !== null && (
                      <span class="font-mono text-[11px] text-tinta-suave">{paso.durationMs} ms</span>
                    )}
                  </div>
                  {paso.error && (
                    <p class="mt-1.5 rounded-[6px] border border-mal/40 bg-mal/5 px-2 py-1 text-[12.5px] text-mal">{paso.error.message}</p>
                  )}
                  <details class="mt-1.5">
                    <summary class="cursor-pointer text-[12px] text-tinta-suave">Entrada / salida</summary>
                    <pre class="mt-1.5 max-h-48 overflow-auto rounded-[6px] border border-borde bg-papel p-2 font-mono text-[11px] leading-relaxed">{JSON.stringify(paso.input, null, 2)}</pre>
                    <pre class="mt-1 max-h-48 overflow-auto rounded-[6px] border border-borde bg-papel p-2 font-mono text-[11px] leading-relaxed">{JSON.stringify(paso.output, null, 2)}</pre>
                  </details>
                </div>
              ))}
              <details>
                <summary class="cursor-pointer text-[12px] text-tinta-suave">Salida final</summary>
                <pre class="mt-1.5 max-h-64 overflow-auto rounded-[6px] border border-borde bg-papel p-2 font-mono text-[11px] leading-relaxed">{JSON.stringify(dryRun.output, null, 2)}</pre>
              </details>
            </div>
          )}
        </Panel>
      )}

      <PanelVersiones
        puenteId={puente.id}
        draft={draft}
        nombrePuente={puente.name}
        slugPuente={puente.slug}
        administra={administra}
        alCambiarBorrador={() => void recargar()}
      />
    </div>
  );
}
