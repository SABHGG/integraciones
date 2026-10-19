//! Punto de entrada de la consola: shell con navegación, sesión (token de
//! usuario o API key) y rol del principal (MVP4).
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { api, clearSesion, getApiKey, getToken, salir as cerrarSesionServidor } from "./api";
import type { Rol } from "./roles";
import { etiquetaRol } from "./roles";
import { Login } from "./auth";
import { Ejecuciones, DetalleEjecucion } from "./executions";
import { EditorPuente, ListaPuentes } from "./bridges";
import { Miembros } from "./members";
import { Credenciales } from "./credentials";
import "./styles.css";

type Ruta =
  | { pantalla: "ejecuciones" }
  | { pantalla: "ejecucion"; id: string }
  | { pantalla: "puentes" }
  | { pantalla: "credenciales" }
  | { pantalla: "puente"; id: string }
  | { pantalla: "miembros" };

function leerRuta(): Ruta {
  const partes = location.pathname.replace(/\/+$/, "").split("/").filter(Boolean);
  if (partes[1] === "executions" && partes[2]) return { pantalla: "ejecucion", id: partes[2] };
  if (partes[1] === "executions") return { pantalla: "ejecuciones" };
  if (partes[1] === "bridges" && partes[2]) return { pantalla: "puente", id: partes[2] };
  if (partes[1] === "members") return { pantalla: "miembros" };
  if (partes[1] === "credentials") return { pantalla: "credenciales" };
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

  const enlace = (rutaDestino: string, texto: string, activo: boolean) => (
    <a
      class={activo ? "activo" : ""}
      href={`/console${rutaDestino}`}
      onClick={(e) => {
        e.preventDefault();
        navegar(rutaDestino);
      }}
    >
      {texto}
    </a>
  );

  return (
    <>
      <header>
        <span class="marca">integraciones</span>
        <nav>
          {enlace("/executions", "Ejecuciones", ruta.pantalla.startsWith("ejecucion"))}
          {enlace("/bridges", "Puentes", ruta.pantalla.startsWith("puente"))}
          {enlace("/members", "Miembros", ruta.pantalla === "miembros")}
          {enlace("/credentials", "Credenciales", ruta.pantalla === "credenciales")}
        </nav>
        <span class="ayuda" style="margin-left: auto;">{etiquetaRol(rol)}</span>
        <button class="secundario salir" onClick={() => void salir()}>Salir</button>
      </header>
      <main>
        {ruta.pantalla === "ejecuciones" && <Ejecuciones navegar={navegar} />}
        {ruta.pantalla === "ejecucion" && <DetalleEjecucion id={ruta.id} />}
        {ruta.pantalla === "puentes" && <ListaPuentes navegar={navegar} />}
        {ruta.pantalla === "puente" && <EditorPuente id={ruta.id} rol={rol} />}
        {ruta.pantalla === "miembros" && <Miembros rol={rol} />}
        {ruta.pantalla === "credenciales" && <Credenciales rol={rol} />}
      </main>
    </>
  );
}

render(<App />, document.getElementById("app")!);
