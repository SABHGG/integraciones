#!/usr/bin/env bash
# Entorno de desarrollo completo en un comando: Postgres portátil, .env local,
# la consola web y el servidor API con su documentación de Scalar. Idempotente:
# seguro de relanzar; un .env existente NUNCA se sobrescribe (el VAULT_KEY
# cifra la bóveda: regenerarlo dejaría las credenciales ilegibles).
#
# Uso:
#   ./scripts/dev.sh                  # postgres + .env + consola + servidor + seed demo
#   ./scripts/dev.sh --no-seed        # lo mismo, sin sembrar el puente demo
#   ./scripts/dev.sh --console-watch  # además reconstruye la consola al vuelo
#
# Al quedar listo expone:
#   http://127.0.0.1:8080/console       consola web (email demo o API key)
#   http://127.0.0.1:8080/docs          UI de Scalar
#   http://127.0.0.1:8080/openapi.json  documento OpenAPI 3.1
#   http://127.0.0.1:8080/x/registro-cliente  (tras el seed)
# Ctrl+C detiene el servidor (Postgres queda corriendo para el próximo arranque).
#
# Nota de desarrollo de la consola: rust-embed en DEBUG lee console/dist/ del
# DISCO en cada petición — `npm run build` + refrescar el navegador, sin
# recompilar el binario. Con --console-watch ni eso: se reconstruye sola.
set -euo pipefail

cd "$(dirname "$0")/.."

SEED=1
CONSOLE_WATCH=0
while [ $# -gt 0 ]; do
    case "$1" in
        --no-seed) SEED=0 ;;
        --console-watch) CONSOLE_WATCH=1 ;;
        *) echo "uso: $0 [--no-seed] [--console-watch]" >&2; exit 2 ;;
    esac
    shift
done

# ── 1. Postgres portátil (idempotente) ──────────────────────────────────
./scripts/dev-postgres.sh

# ── 2. .env local: se crea solo si falta ────────────────────────────────
if [ ! -f .env ]; then
    echo ">> creando .env local..."
    VAULT_KEY_NUEVA=$(
        openssl rand -hex 32 2>/dev/null \
            || od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
    )
    cat > .env <<EOF
DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones
PORT=8080
RUST_LOG=info
VAULT_KEY=${VAULT_KEY_NUEVA}
EOF
    echo ">> .env creado con VAULT_KEY nueva (guárdala: cifra la bóveda)."
else
    echo ">> .env existente: se conserva tal cual."
fi

# Cargar el .env para el propio script (puerto del health check, VAULT_KEY del
# seed); el servidor además lo lee con dotenvy.
set -a
. ./.env
set +a

if [ -z "${VAULT_KEY:-}" ]; then
    echo ">> AVISO: tu .env no define VAULT_KEY; la bóveda usará una clave" >&2
    echo ">> efímera por proceso y los secretos no sobrevivirán a un reinicio." >&2
elif ! printf '%s' "$VAULT_KEY" | grep -qE '^[0-9a-f]{64}$'; then
    # Un VAULT_KEY no-hex jamás pudo cifrar nada: el vault cae a clave efímera
    # (crates/api/src/vault.rs), así que reemplazarlo no pierde ningún secreto.
    echo ">> VAULT_KEY no es hex de 64 (¿copiaste el literal \$(openssl …)?)." >&2
    echo ">> La reemplazo por una real: sin ella la bóveda caía a clave efímera." >&2
    sed -i "s|^VAULT_KEY=.*|VAULT_KEY=$(openssl rand -hex 32)" .env
    . ./.env   # recarga con la clave real recién escrita
    echo ">> VAULT_KEY real escrita en .env."
fi

PORT="${PORT:-8080}"
BASE_URL="http://127.0.0.1:${PORT}"

# Puerto ocupado por otro proceso (p. ej. un binario zombi de una ejecución
# anterior: matar el wrapper cargo run no siempre mata al binario hijo):
# fallar temprano con el PID culpable en vez de chocar con AddrInUse a ciegas.
if command -v ss >/dev/null 2>&1 \
    && ss -ltn "( sport = :$PORT )" 2>/dev/null | grep -q LISTEN; then
    echo ">> el puerto $PORT ya está ocupado por otro proceso:" >&2
    ss -ltnp "( sport = :$PORT )" 2>/dev/null | grep LISTEN >&2 || true
    echo ">> mátalo (kill <PID>) o pon otro PORT en .env y relanza." >&2
    exit 1
fi

# ── 3. Consola web (MVP3): assets de console/dist bajo /console ──────────
# Sin Node el binario sigue compilando (build.rs genera un stub) y /console
# sirve una página que lo explica; con Node, la consola se construye aquí.
if ! command -v npm >/dev/null 2>&1; then
    echo ">> AVISO: sin npm no se construye la consola; /console servirá el stub." >&2
elif [ ! -d console/node_modules ]; then
    echo ">> instalando dependencias de la consola (primera vez)..."
    if ! (cd console && npm ci --no-audit --no-fund) \
        && ! (cd console && npm install --no-audit --no-fund); then
        echo ">> AVISO: npm install falló; /console servirá el stub." >&2
    fi
fi

if command -v npm >/dev/null 2>&1 && [ -d console/node_modules ]; then
    if [ ! -f console/dist/index.html ] \
        || [ -n "$(find console/src console/index.html console/vite.config.ts \
                    console/tsconfig.json -newer console/dist/index.html \
                    -print -quit 2>/dev/null)" ]; then
        echo ">> construyendo la consola..."
        if ! (cd console && npm run build); then
            echo ">> AVISO: el build de la consola falló; /console servirá la última versión" >&2
            echo ">> buena (o el stub si nunca se construyó)." >&2
        fi
    else
        echo ">> consola al día (console/dist)."
    fi
fi

if [ "$CONSOLE_WATCH" = 1 ]; then
    if command -v npm >/dev/null 2>&1 && [ -d console/node_modules ]; then
        echo ">> watch de la consola activo: al editar console/src se reconstruye sola."
        echo ">> (refresca el navegador; el binario NO se recompila.)"
        (cd console && npx vite build --watch) &
        CONSOLE_PID=$!
    else
        echo ">> AVISO: --console-watch sin npm/node_modules: se ignora." >&2
        CONSOLE_PID=""
    fi
else
    CONSOLE_PID=""
fi

# ── 4. Servidor: compila la 1.ª vez (~1 min) y arranca en background ───
echo ">> compilando y arrancando el servidor en ${BASE_URL}..."
cargo run -p api &
SERVIDOR=$!
trap 'kill "$SERVIDOR" "${CONSOLE_PID:-}" 2>/dev/null || true' EXIT INT TERM

# Espera al health check sin bloquear más de 3 minutos (compilación fría).
LISTO=0
for _ in $(seq 1 180); do
    if curl -fsS "${BASE_URL}/healthz" >/dev/null 2>&1; then
        LISTO=1
        break
    fi
    if ! kill -0 "$SERVIDOR" 2>/dev/null; then
        echo ">> el servidor murió al arrancar; revisa el log de arriba" >&2
        exit 1
    fi
    sleep 1
done
if [ "$LISTO" != 1 ]; then
    echo ">> el servidor no respondió /healthz en 180s" >&2
    exit 1
fi

echo
echo ">> API lista:   ${BASE_URL}"
echo ">> Consola:     ${BASE_URL}/console"
echo ">>   email: demo@integraciones.dev / demo-password-2026 (owner del workspace demo)"
echo ">>   o API key: demo-key-1 (acceso de automatización, actúa como owner)"
echo ">> Docs Scalar: ${BASE_URL}/docs"
echo ">> OpenAPI:     ${BASE_URL}/openapi.json"
echo

# ── 4. Puente demo publicado y listo para invocar (opcional) ───────────
if [ "$SEED" = 1 ]; then
    ./scripts/seed-demo.sh || {
        echo ">> seed-demo falló; el servidor sigue arriba sin datos demo" >&2
    }
    echo
    echo ">> key demo: 'Authorization: Bearer demo-key-1' (workspace 'demo')"
    echo ">> consola:  entra en ${BASE_URL}/console con demo@integraciones.dev /"
    echo ">>           demo-password-2026 (owner) o la key demo 'demo-key-1'"
    echo ">> prueba:   curl -s -X POST ${BASE_URL}/x/registro-cliente \\"
    echo "               -H 'content-type: application/json' -d '{\"items\": [\"cafe\", \"mate\"]}'"
    echo
fi

echo ">> Ctrl+C para detener el servidor." 
wait "$SERVIDOR"
