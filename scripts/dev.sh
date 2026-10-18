#!/usr/bin/env bash
# Entorno de desarrollo completo en un comando: Postgres portátil, .env local
# y el servidor API con su documentación de Scalar. Idempotente: seguro de
# relanzar; un .env existente NUNCA se sobrescribe (el VAULT_KEY cifra la
# bóveda: regenerarlo dejaría las credenciales ilegibles).
#
# Uso:
#   ./scripts/dev.sh              # postgres + .env + servidor + seed demo
#   ./scripts/dev.sh --no-seed    # lo mismo, sin sembrar el puente demo
#
# Al quedar listo expone:
#   http://127.0.0.1:8080/docs          UI de Scalar
#   http://127.0.0.1:8080/openapi.json  documento OpenAPI 3.1
#   http://127.0.0.1:8080/x/registro-cliente  (tras el seed)
# Ctrl+C detiene el servidor (Postgres queda corriendo para el próximo arranque).
set -euo pipefail

cd "$(dirname "$0")/.."

SEED=1
while [ $# -gt 0 ]; do
    case "$1" in
        --no-seed) SEED=0 ;;
        *) echo "uso: $0 [--no-seed]" >&2; exit 2 ;;
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

# ── 3. Servidor: compila la 1.ª vez (~1 min) y arranca en background ───
echo ">> compilando y arrancando el servidor en ${BASE_URL}..."
cargo run -p api &
SERVIDOR=$!
trap 'kill "$SERVIDOR" 2>/dev/null || true' EXIT INT TERM

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
    echo ">> prueba:   curl -s -X POST ${BASE_URL}/x/registro-cliente \\"
    echo "               -H 'content-type: application/json' -d '{\"items\": [\"cafe\", \"mate\"]}'"
    echo
fi

echo ">> Ctrl+C para detener el servidor."
wait "$SERVIDOR"
