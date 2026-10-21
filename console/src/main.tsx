//! Punto de entrada de la consola: shell con navegación, sesión (token de
//! usuario o API key) y rol del principal (MVP4).
import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { driver, type Driver } from "driver.js";
import { api, clearSesion, getApiKey, getToken, salir as cerrarSesionServidor } from "./lib/api";
import type { Rol } from "./lib/roles";
import { etiquetaRol } from "./lib/roles";
import { leerTourVisto, marcarTourVisto, movimientoReducido, pasosDisponibles } from "./lib/tour";
import { Login } from "./paginas/auth";
import { Ejecuciones, DetalleEjecucion } from "./paginas/executions";
import { EditorPuente, ListaPuentes } from "./paginas/bridges";
import { Miembros } from "./paginas/members";
import { Credenciales } from "./paginas/credentials";
import { BibliotecaBloques, DetalleBloque } from "./paginas/blocks";
// La hoja de driver.js va empaquetada (D1: nunca CDN) y antes que el tema para
// que los overrides del popover en tema.css ganen la cascada.
import "driver.js/dist/driver.css";
import "./tema.css";

type Ruta =
  | { pantalla: "ejecuciones" }
  | { pantalla: "ejecucion"; id: string }
  | { pantalla: "puentes" }
  | { pantalla: "credenciales" }
  | { pantalla: "bloques" }
  | { pantalla: "puente"; id: string }
  | { pantalla: "bloque"; id: string }
  | { pantalla: "miembros" };

function leerRuta(): Ruta {
  const partes = location.pathname.replace(/\/+$/, "").split("/").filter(Boolean);
  if (partes[1] === "executions" && partes[2]) return { pantalla: "ejecucion", id: partes[2] };
  if (partes[1] === "executions") return { pantalla: "ejecuciones" };
  if (partes[1] === "bridges" && partes[2]) return { pantalla: "puente", id: partes[2] };
  if (partes[1] === "members") return { pantalla: "miembros" };
  if (partes[1] === "credentials") return { pantalla: "credenciales" };
  if (partes[1] === "blocks" && partes[2]) return { pantalla: "bloque", id: partes[2] };
  if (partes[1] === "blocks") return { pantalla: "bloques" };
  return { pantalla: "puentes" };
}

function navegar(ruta: string): void {
  history.pushState({ app: true }, "", `/console${ruta}`);
  dispatchEvent(new PopStateEvent("popstate"));
}

/// Identidad del whoami: rol del principal y workspace activo (el tour indexa
/// su estado por workspace).
interface Identidad {
  principal: { type: string; role?: string };
  activeWorkspace?: { id: string; slug: string; name: string; role: string } | null;
}

/// Primer elemento **visible** del selector: el shell es responsive y el mismo
/// paso puede existir dos veces (sidebar oculto en móvil, barra superior oculta
/// en escritorio). Un elemento ausente o no visible se descarta (D6).
function elementoVisible(selector: string): Element | null {
  for (const elemento of document.querySelectorAll(selector)) {
    if (elemento instanceof HTMLElement && elemento.getClientRects().length > 0) return elemento;
  }
  return null;
}

function App(): preact.ComponentChildren {
  const [conSesion, setConSesion] = useState<boolean>(Boolean(getToken() || getApiKey()));
  const [rol, setRol] = useState<Rol | null>(null);
  // `undefined` = whoami pendiente; `null` = sesión sin workspace fijado.
  const [workspace, setWorkspace] = useState<string | null | undefined>(undefined);
  const [ruta, setRuta] = useState<Ruta>(leerRuta());
  const drv = useRef<Driver | null>(null);
  const tourAutomaticoLanzado = useRef(false);

  // Al recargar, el rol se recupera del whoami (el token vive en localStorage).
  useEffect(() => {
    if (!conSesion) return;
    void api<Identidad>("GET", "/whoami")
      .then((identidad) => {
        setRol((identidad.principal.role as Rol) ?? null);
        setWorkspace(identidad.activeWorkspace?.id ?? null);
      })
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

  // D7: una sola instancia viva; se destruye al navegar (este efecto se limpia
  // en cada cambio de ruta) y al desmontar el shell (limpieza del mismo efecto).
  useEffect(() => {
    return () => {
      drv.current?.destroy();
      drv.current = null;
    };
  }, [ruta]);

  // Al cerrar sesión el shell se sustituye por el login: el tour no sobrevive.
  useEffect(() => {
    if (conSesion) return;
    drv.current?.destroy();
    drv.current = null;
  }, [conSesion]);

  /// Construye una instancia nueva de driver.js con los pasos aplicables al rol
  /// de la sesión, ya filtrados por existencia en el DOM (D6). Relanzar siempre
  /// arranca desde el primer paso, esté o no marcado como visto (D4/2.3).
  const lanzarTour = () => {
    drv.current?.destroy();
    drv.current = null;
    const pasos = pasosDisponibles(rol, (selector) => elementoVisible(selector) !== null);
    // Sin destinos no se abre un popover vacío ni se marca el tour como visto.
    if (pasos.length === 0) return;
    const reducido = movimientoReducido();
    // El velo del tour es la tinta del sistema, leída del tema (D5): el color
    // no se inventa aquí, se toma del token vigente.
    const tinta =
      getComputedStyle(document.documentElement).getPropertyValue("--color-tinta").trim() ||
      "#14352d";
    // D6 reforzado: el nodo se resuelve UNA vez al construir los pasos (su
    // existencia ya se filtró en `pasosDisponibles`) y viaja como Element
    // estático. Con un getter re-evaluado, driver.js posicionaba el popover
    // contra el rect previo al scrollIntoView del paso anterior y el popover
    // quedaba descolgado del elemento (p. ej. el filtro de estados).
    const instancia = driver({
      steps: pasos.flatMap((paso) => {
        const nodo = elementoVisible(paso.elemento);
        return nodo
          ? [{ element: nodo, popover: { title: paso.titulo, description: paso.descripcion } }]
          : [];
      }),
      popoverClass: "tour-rastro",
      showProgress: true,
      progressText: "{{current}} de {{total}}",
      nextBtnText: "Siguiente",
      prevBtnText: "Atrás",
      doneBtnText: "Listo",
      closeBtnLabel: "Cerrar el tutorial",
      overlayColor: tinta,
      overlayOpacity: 0.55,
      stagePadding: 4,
      stageRadius: 8,
      allowClose: true,
      allowKeyboardControl: true,
      animate: !reducido,
      smoothScroll: !reducido,
      // El scrollIntoView del paso puede seguir en vuelo cuando driver.js
      // posiciona el popover (y su handler de scroll no lo realinea): unos
      // refresh tras el asentamiento cuestan nada y dejan el popover pegado
      // al elemento resaltado.
      onHighlighted: () => {
        requestAnimationFrame(() => drv.current?.refresh());
        setTimeout(() => drv.current?.refresh(), 250);
        setTimeout(() => drv.current?.refresh(), 700);
      },
      onDestroyed: (_elemento, _paso, opciones) => {
        drv.current = null;
        // Navegar o desmontar destruye con motivo "api": eso no es haberlo visto.
        if (opciones.reason !== "api") marcarTourVisto(workspace);
      },
    });
    drv.current = instancia;
    instancia.drive();
  };

  // 2.2: auto-lanzado una sola vez por carga, solo si este navegador no lo vio
  // para el workspace vigente, y tras el primer render de la página.
  useEffect(() => {
    if (!conSesion || rol === null || workspace === undefined) return;
    if (tourAutomaticoLanzado.current) return;
    tourAutomaticoLanzado.current = true;
    if (leerTourVisto(workspace)) return;
    const id = setTimeout(lanzarTour, 250);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conSesion, rol, workspace]);

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
    { ruta: "/blocks", texto: "Bloques", activo: ruta.pantalla === "bloques" || ruta.pantalla === "bloque" },
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
          {/* D4: el tour es acción de sesión, no de página: vive junto a Salir. */}
          <button
            class="rounded-[7px] border border-papel/28 px-2.5 py-2 text-left text-[13.5px] text-papel/85 transition-colors hover:bg-papel/8 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel"
            onClick={lanzarTour}
          >
            Ver tutorial
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
          class="rounded-[7px] border border-papel/28 px-2.5 py-1.5 text-[13px] text-papel/85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel"
          onClick={lanzarTour}
        >
          Ver tutorial
        </button>
        <button
          class="rounded-[7px] border border-papel/28 px-2.5 py-1.5 text-[13px] text-papel/85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-papel"
          onClick={() => void salir()}
        >
          Salir
        </button>
      </header>

      <main class="min-w-0 flex-1 px-4 py-5 md:px-7">
        <div class="mx-auto max-w-[1240px]" data-pantalla={ruta.pantalla}>
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
                : <EditorPuente id={ruta.id} rol={rol} navegar={navegar} />}
            </>
          )}
          {ruta.pantalla === "bloque" && (
            <DetalleBloque id={ruta.id} rol={rol} navegar={navegar} />
          )}
          {ruta.pantalla === "miembros" && <Miembros rol={rol} />}
          {ruta.pantalla === "credenciales" && <Credenciales rol={rol} />}
          {ruta.pantalla === "bloques" && <BibliotecaBloques rol={rol} navegar={navegar} />}
        </div>
      </main>
    </div>
  );
}

render(<App />, document.getElementById("app")!);
