//! Punto de entrada de la consola: shell con navegación y sesión por API key.
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { getKey, clearKey } from "./api";
import { Login } from "./auth";
import { Ejecuciones, DetalleEjecucion } from "./executions";
import { EditorPuente, ListaPuentes } from "./bridges";
import "./styles.css";

type Ruta = { pantalla: "ejecuciones" } | { pantalla: "ejecucion"; id: string } | { pantalla: "puentes" } | { pantalla: "puente"; id: string };

function leerRuta(): Ruta {
  const partes = location.pathname.replace(/\/+$/, "").split("/").filter(Boolean);
  // /console/executions/{id} · /console/bridges/{id}
  if (partes[1] === "executions" && partes[2]) return { pantalla: "ejecucion", id: partes[2] };
  if (partes[1] === "executions") return { pantalla: "ejecuciones" };
  if (partes[1] === "bridges" && partes[2]) return { pantalla: "puente", id: partes[2] };
  return { pantalla: "puentes" };
}

function navegar(ruta: string): void {
  history.pushState(null, "", `/console${ruta}`);
  dispatchEvent(new PopStateEvent("popstate"));
}

function App(): preact.ComponentChildren {
  const [conSesion, setConSesion] = useState<boolean>(Boolean(getKey()));
  const [ruta, setRuta] = useState<Ruta>(leerRuta());

  useEffect(() => {
    const alMover = () => setRuta(leerRuta());
    addEventListener("popstate", alMover);
    return () => removeEventListener("popstate", alMover);
  }, []);

  if (!conSesion) {
    return <Login alEntrar={() => setConSesion(true)} />;
  }

  const salir = () => {
    clearKey();
    setConSesion(false);
  };

  return (
    <>
      <header>
        <span class="marca">integraciones</span>
        <nav>
          <a class={ruta.pantalla.startsWith("ejecucion") ? "activo" : ""} href="/console/executions" onClick={(e) => { e.preventDefault(); navegar("/executions"); }}>
            Ejecuciones
          </a>
          <a class={ruta.pantalla.startsWith("puente") ? "activo" : ""} href="/console/bridges" onClick={(e) => { e.preventDefault(); navegar("/bridges"); }}>
            Puentes
          </a>
        </nav>
        <button class="secundario salir" onClick={salir}>Salir</button>
      </header>
      <main>
        {ruta.pantalla === "ejecuciones" && <Ejecuciones navegar={navegar} />}
        {ruta.pantalla === "ejecucion" && <DetalleEjecucion id={ruta.id} />}
        {ruta.pantalla === "puentes" && <ListaPuentes navegar={navegar} />}
        {ruta.pantalla === "puente" && <EditorPuente id={ruta.id} />}
      </main>
    </>
  );
}

render(<App />, document.getElementById("app")!);
