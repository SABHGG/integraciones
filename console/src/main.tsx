//! Punto de entrada de la consola: shell con navegación, sesión (token de
//! usuario o API key) y rol del principal (MVP4).
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { api, clearSesion, getApiKey, getToken, salir as cerrarSesionServidor } from "./lib/api";
import type { Rol } from "./lib/roles";
import { etiquetaRol } from "./lib/roles";
import { Login } from "./paginas/auth";
import { Ejecuciones, DetalleEjecucion } from "./paginas/executions";
import { EditorPuente, ListaPuentes } from "./paginas/bridges";
import { Miembros } from "./paginas/members";
import { Credenciales } from "./paginas/credentials";
import { BibliotecaBloques } from "./paginas/blocks";
import "./tema.css";

type Ruta =
  | { pantalla: "ejecuciones" }
  | { pantalla: "ejecucion"; id: string }
  | { pantalla: "puentes" }
  | { pantalla: "credenciales" }
  | { pantalla: "bloques" }
  | { pantalla: "puente"; id: string }
  | { pantalla: "miembros" };

function leerRuta(): Ruta {
  const partes = location.pathname.replace(/\/+$/, "").split("/").filter(Boolean);
  if (partes[1] === "executions" && partes[2]) return { pantalla: "ejecucion", id: partes[2] };
  if (partes[1] === "executions") return { pantalla: "ejecuciones" };
  if (partes[1] === "bridges" && partes[2]) return { pantalla: "puente", id: partes[2] };
  if (partes[1] === "members") return { pantalla: "miembros" };
  if (partes[1] === "credentials") return { pantalla: "credenciales" };
  if (partes[1] === "blocks") return { pantalla: "bloques" };
  return { pantalla: "puentes" };
}

function navegar(ruta: string): void {
  history.pushState(null, "", `/console${ruta}`);
  dispatchEvent(new PopStateEvent("popstate"));
}

function App(): preact.ComponentChildren {
  const [conSesion, setConSesion] = useState<boolean>(Boolean(getToken() || getApiKey()));
  const [rol, setRol] = useState<Rol | null>(null);
  const [ruta, setRuta] = useState<Ruta>(leerRuta());

  // Al recargar, el rol se recupera del whoami (el token vive en localStorage).
  useEffect(() => {
    if (!conSesion) return;
    void api<{ principal: { type: string; role?: string } }>("GET", "/whoami")
      .then((identidad) => setRol((identidad.principal.role as Rol) ?? null))
      .catch(() => {
        // Token muerto: cerrar y volver al login.
        clearSesion();
        setConSesion(false);
      });
  }, [conSesion]);

  useEffect(() => {
    const alMover = () => setRuta(leerRuta());
    addEventListener("popstate", alMover);
    return () => removeEventListener("popstate", alMover);
  }, []);

  if (!conSesion) {
    return <Login alEntrar={(rol) => { setRol(rol as Rol); setConSesion(true); }} />;
  }

  const salir = async () => {
    await cerrarSesionServidor(); // revoca el token en el servidor si es de usuario
    setConSesion(false);
    setRol(null);
  };

  const secciones = [
    { ruta: "/executions", texto: "Ejecuciones", activo: ruta.pantalla.startsWith("ejecucion") },
    { ruta: "/bridges", texto: "Puentes", activo: ruta.pantalla === "puentes" || ruta.pantalla === "puente" },
    { ruta: "/members", texto: "Miembros", activo: ruta.pantalla === "miembros" },
    { ruta: "/credentials", texto: "Credenciales", activo: ruta.pantalla === "credenciales" },
    { ruta: "/blocks", texto: "Bloques", activo: ruta.pantalla === "bloques" },
  ];

  const enlace = (s: (typeof secciones)[number]) => (
    <a
      href={`/console${s.ruta}`}
      onClick={(e) => {
        e.preventDefault();
        navegar(s.ruta);
      }}
      class={`rounded-[7px] px-2.5 py-2 text-[13.5px] transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel ${
        s.activo
          ? "bg-papel/12 font-semibold text-papel"
          : "font-medium text-papel/70 hover:bg-papel/6 hover:text-papel"
      }`}
    >
      {s.texto}
    </a>
  );

  const seccionActual = secciones.find((s) => s.activo) ?? secciones[0];

  return (
    <div class="flex min-h-screen flex-col bg-papel md:flex-row">
      {/* Sidebar de tinta: el único gesto de identidad del sistema (desktop). */}
      <aside class="hidden w-[218px] shrink-0 flex-col bg-tinta p-4 pt-5 md:sticky md:top-0 md:flex md:h-screen">
        <div class="px-1.5 pb-6 text-[16px] font-bold tracking-tight text-papel">integraciones</div>
        <nav class="flex flex-col gap-0.5" aria-label="Secciones">
          {secciones.map((s) => enlace(s))}
        </nav>
        <div class="mt-auto flex flex-col gap-2.5 border-t border-dashed border-papel/25 pt-4">
          <span class="self-start rounded-[7px] border border-papel/28 px-2.5 py-1.5 font-mono text-[11.5px] text-papel/75">
            {etiquetaRol(rol)}
          </span>
          <button
            class="rounded-[7px] border border-papel/28 px-2.5 py-2 text-left text-[13.5px] text-papel/85 transition-colors hover:bg-papel/8 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel"
            onClick={() => void salir()}
          >
            Salir
          </button>
        </div>
      </aside>

      {/* Barra superior con selector: la misma navegación en móvil. */}
      <header class="flex items-center gap-3 bg-tinta px-4 py-3 md:hidden">
        <span class="text-[15px] font-bold tracking-tight text-papel">integraciones</span>
        <select
          class="ml-auto max-w-[55%] rounded-[7px] border border-papel/28 bg-transparent px-2 py-1.5 text-[13px] text-papel focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel"
          value={seccionActual.ruta}
          onChange={(e) => navegar((e.target as HTMLSelectElement).value)}
          aria-label="Sección"
        >
          {secciones.map((s) => (
            <option key={s.ruta} value={s.ruta} class="text-tinta">
              {s.texto}
            </option>
          ))}
        </select>
        <button
          class="rounded-[7px] border border-papel/28 px-2.5 py-1.5 text-[13px] text-papel/85"
          onClick={() => void salir()}
        >
          Salir
        </button>
      </header>

      <main class="min-w-0 flex-1 px-4 py-5 md:px-7">
        <div class="mx-auto max-w-[1240px]">
          {/* Páginas migradas al sistema El rastro. */}
          {(ruta.pantalla === "ejecuciones" || ruta.pantalla === "ejecucion") && (
            <>
              {ruta.pantalla === "ejecuciones"
                ? <Ejecuciones navegar={navegar} />
                : <DetalleEjecucion id={ruta.id} />}
            </>
          )}
          {(ruta.pantalla === "puentes" || ruta.pantalla === "puente") && (
            <>
              {ruta.pantalla === "puentes"
                ? <ListaPuentes navegar={navegar} rol={rol} />
                : <EditorPuente id={ruta.id} rol={rol} />}
            </>
          )}
          {ruta.pantalla === "miembros" && <Miembros rol={rol} />}
          {ruta.pantalla === "credenciales" && <Credenciales rol={rol} />}
          {ruta.pantalla === "bloques" && <BibliotecaBloques rol={rol} />}
        </div>
      </main>
    </div>
  );
}

render(<App />, document.getElementById("app")!);
