#!/usr/bin/env bash
# Siembra el puente demo del change add-mvp1-core (task 5.1) contra un
# servidor corriendo (por defecto http://127.0.0.1:8080).
#
# Qué crea (idempotente: si el puente demo ya está publicado, no repite nada):
#   - workspace `demo` + API key `demo-key-1` (cruda, solo para desarrollo).
#   - Bóveda: credencial `pago-api`.
#   - Biblioteca: bloques `http-get`, `http-post` (built-in, versiones sin
#     código) y `enriquece-cliente` (custom_logic con TypeScript publicado).
#   - Puente `registro-cliente` de 3 bloques encadenados:
#       http-get → custom_logic → http-post
#     publicado como v1 y listo para invocar en POST /x/registro-cliente.
#
# Uso:
#   ./scripts/dev-postgres.sh
#   export VAULT_KEY=$(openssl rand -hex 32)   # o deja que el servidor use una efímera
#   cargo run -p api &
#   ./scripts/seed-demo.sh
#   curl -s -X POST localhost:8080/x/registro-cliente \
#        -H 'content-type: application/json' \
#        -d '{"items": ["cafe", "mate"]}'
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
DATABASE_URL="${DATABASE_URL:-postgres://integraciones@127.0.0.1:5433/integraciones}"
DEMO_BASE_URL="${DEMO_BASE_URL:-https://httpbin.org}"
DEMO_KEY="demo-key-1"
WS_SLUG="demo"
CURL="curl -sf"

if [ -z "${VAULT_KEY:-}" ]; then
    echo ">> AVISO: VAULT_KEY no definida; el servidor usará una clave efímera y" >&2
    echo ">> la credencial del demo dejará de descifrarse si reinicias el proceso." >&2
fi

# ── psql del Postgres portátil (o del PATH) ─────────────────────────────
PG_BIN="${PG_SETUP_DIR:-/tmp/pg-setup}/pg18/bin"
if [ -x "$PG_BIN/psql" ]; then
    PSQL="$PG_BIN/psql"
else
    PSQL="psql"
fi
export LD_LIBRARY_PATH="${PG_SETUP_DIR:-/tmp/pg-setup}/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# ── workspace + API key (bootstrap, único paso SQL) ────────────────────
UUID_RE='[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
KEY_HASH=$(printf '%s' "$DEMO_KEY" | sha256sum | cut -d' ' -f1)
WORKSPACE_ID=$(
    $PSQL "$DATABASE_URL" -tAc "
        INSERT INTO workspaces (id, name, slug)
        VALUES (gen_random_uuid(), 'Workspace demo', '$WS_SLUG')
        ON CONFLICT (slug) DO UPDATE SET slug = EXCLUDED.slug
        RETURNING id" | grep -oE "$UUID_RE" | head -1
)
$PSQL "$DATABASE_URL" -tAc "
    INSERT INTO api_keys (id, workspace_id, name, key_hash)
    VALUES (gen_random_uuid(), '$WORKSPACE_ID', 'demo-key', '$KEY_HASH')
    ON CONFLICT DO NOTHING" >/dev/null
echo ">> workspace demo: $WORKSPACE_ID"

AUTH="Authorization: Bearer $DEMO_KEY"
CT="content-type: application/json"
post() { $CURL -s -X POST "$BASE_URL$1" -H "$AUTH" -H "$CT" -d "$2"; }
patch_() { $CURL -s -X PATCH "$BASE_URL$1" -H "$AUTH" -H "$CT" -d "$2"; }
jget() { python3 -c "import json,sys; d=json.load(sys.stdin); print(d$1)"; }

# ¿Ya está sembrado? (puente publicado ⇒ nada por hacer)
BRIDGE_ID=$($PSQL "$DATABASE_URL" -tAc "
    SELECT id FROM bridges WHERE workspace_id = '$WORKSPACE_ID' AND slug = 'registro-cliente'" | tr -d '[:space:]')
if [ -n "$BRIDGE_ID" ] && [ -n "$($PSQL "$DATABASE_URL" -tAc "
        SELECT 1 FROM bridges WHERE id = '$BRIDGE_ID' AND latest_version IS NOT NULL")" ]; then
    echo ">> el puente demo ya está publicado (id $BRIDGE_ID): nada por hacer."
    echo ">> key: $DEMO_KEY"
    exit 0
fi

# ── bóveda ──────────────────────────────────────────────────────────────
HAS_CRED=$($PSQL "$DATABASE_URL" -tAc "
    SELECT 1 FROM credentials WHERE workspace_id = '$WORKSPACE_ID' AND name = 'pago-api'" | tr -d '[:space:]')
if [ -z "$HAS_CRED" ]; then
    post "/api/v1/credentials" '{"name": "pago-api", "secret": "sk_demo_secreto", "description": "key del proveedor (demo)"}' >/dev/null
    echo ">> credencial pago-api sembrada"
fi

# ── biblioteca: tres bloques con su versión publicada ──────────────────
crear_bloque() { # slug block_type
    post "/api/v1/blocks" "{\"slug\": \"$1\", \"name\": \"$2\", \"blockType\": \"$3\", \"category\": \"$4\", \"description\": \"$5\", \"icon\": \"$6\"}" | jget "['id']"
}
publicar_version() { # block_id body
    post "/api/v1/blocks/$1/versions" "$2" | jget "['id']"
}
bloque_publicado() { # slug → imprime (block_id, version_id) si ya tiene versión
    $PSQL "$DATABASE_URL" -tAc "
        SELECT b.id || ' ' || bv.id
        FROM blocks b JOIN block_versions bv ON bv.block_id = b.id AND bv.version = b.latest_version
        WHERE b.workspace_id = '$WORKSPACE_ID' AND b.slug = '$1'" | tr -d '[:space:]'
}

GET_IDS=$(bloque_publicado "http-get")
if [ -z "$GET_IDS" ]; then
    GET_ID=$(crear_bloque "http-get" "HTTP GET" "http" "http" "Llama a una URL por GET" "globe")
    GET_VER=$(publicar_version "$GET_ID" '{"outputSchema": {"type": "object"}}')
else
    GET_ID="${GET_IDS% *}"
    GET_VER="${GET_IDS#* }"
fi

POST_IDS=$(bloque_publicado "http-post")
if [ -z "$POST_IDS" ]; then
    POST_ID=$(crear_bloque "http-post" "HTTP POST" "http" "http" "Llama a una URL por POST con body JSON" "send")
    POST_VER=$(publicar_version "$POST_ID" '{"outputSchema": {"type": "object"}}')
else
    POST_ID="${POST_IDS% *}"
    POST_VER="${POST_IDS#* }"
fi

CODIGO_CUSTOM='export default (input, ctx) => { const origen = input.usuario ?? {}; const nombre = origen.nombre ?? origen.url ?? "demo"; ctx.log("info", "enriqueciendo " + nombre); return { cliente: String(nombre).toUpperCase(), procesadoEn: new Date().toISOString() }; };'
CUSTOM_IDS=$(bloque_publicado "enriquece-cliente")
if [ -z "$CUSTOM_IDS" ]; then
    CUSTOM_ID=$(crear_bloque "enriquece-cliente" "Enriquece cliente" "custom_logic" "transform" "Pone el nombre en mayúsculas y agrega la marca de tiempo" "wand")
    CUSTOM_VER=$(publicar_version "$CUSTOM_ID" "{\"code\": $(python3 -c "import json;print(json.dumps('''$CODIGO_CUSTOM'''))"), \"inputSchema\": {\"type\": \"object\", \"properties\": {\"usuario\": {\"type\": \"object\"}}}, \"outputSchema\": {\"type\": \"object\", \"properties\": {\"cliente\": {\"type\": \"string\"}, \"procesadoEn\": {\"type\": \"string\"}}, \"required\": [\"cliente\", \"procesadoEn\"]}}")
else
    CUSTOM_ID="${CUSTOM_IDS% *}"
    CUSTOM_VER="${CUSTOM_IDS#* }"
fi
echo ">> bloques: http-get=$GET_ID http-post=$POST_ID enriquece-cliente=$CUSTOM_ID"

# ── puente: borrador → validar → publicar ──────────────────────────────
if [ -z "$BRIDGE_ID" ]; then
    BRIDGE_ID=$(post "/api/v1/bridges" '{"slug": "registro-cliente", "name": "Registro de cliente"}' | jget "['id']")
fi

BORRADOR=$(python3 - "$DEMO_BASE_URL" "$GET_ID" "$GET_VER" "$CUSTOM_ID" "$CUSTOM_VER" "$POST_ID" "$POST_VER" <<'PYEOF'
import json, sys
base, get_id, get_ver, custom_id, custom_ver, post_id, post_ver = sys.argv[1:]
print(json.dumps({
    "trigger": {"type": "http", "method": "POST", "path": "/x/registro-cliente"},
    "inputSchema": {
        "type": "object",
        "properties": {"items": {"type": "array", "items": {"type": "string"}}},
        "required": ["items"],
    },
    "blocks": [
        {
            "id": "b1", "name": "traeUsuario",
            "blockId": get_id, "blockVersionId": get_ver,
            "config": {
                "url": base + "/get",
                "method": "GET",
                "headers": {"X-Api-Key": "{$credential:pago-api}"},
            },
        },
        {
            "id": "b2", "name": "enriquece",
            "blockId": custom_id, "blockVersionId": custom_ver,
            "input": {"usuario": "$.traeUsuario"},
        },
        {
            "id": "b3", "name": "creaOrden",
            "blockId": post_id, "blockVersionId": post_ver,
            "config": {
                "url": base + "/post",
                "method": "POST",
                "body": {"cliente": "$.enriquece.cliente", "items": "$.trigger.body.items"},
            },
        },
    ],
}))
PYEOF
)
patch_ "/api/v1/bridges/$BRIDGE_ID" "$BORRADOR" >/dev/null
VALID=$($CURL -s -X POST "$BASE_URL/api/v1/bridges/$BRIDGE_ID/validate" -H "$AUTH" -H "$CT")
[ "$(echo "$VALID" | jget "['valid']")" = "True" ] || {
    echo "!! el borrador del puente demo no valida: $VALID" >&2
    exit 1
}
$CURL -s -X POST "$BASE_URL/api/v1/bridges/$BRIDGE_ID/versions" -H "$AUTH" -H "$CT" >/dev/null

echo ""
echo "======================================================================"
echo " Puente demo listo:  POST $BASE_URL/x/registro-cliente"
echo " API key:            $DEMO_KEY"
echo " Ejemplo:"
echo "   curl -s -X POST $BASE_URL/x/registro-cliente \\"
echo "        -H 'content-type: application/json' \\"
echo "        -d '{\"items\": [\"cafe\", \"mate\"]}'"
echo " Ejecuciones (con la misma key):"
echo "   curl -s -H 'Authorization: Bearer $DEMO_KEY' \\"
echo "        $BASE_URL/api/v1/executions | python3 -m json.tool"
echo "======================================================================"
