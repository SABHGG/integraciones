#!/usr/bin/env bash
# Postgres portátil para desarrollo local sin docker y sin root.
# Descarga binarios de theseus-rs/postgresql-binaries + libs (libxml2, libicu)
# extraídas de .deb, inicializa un cluster en $PG_DIR y arranca en el puerto 5433.
set -euo pipefail

PG_VERSION="18.6.0"
BASE="${PG_SETUP_DIR:-/tmp/pg-setup}"
LIB="$BASE/lib"
DATA="$BASE/pgdata"
export LD_LIBRARY_PATH="$LIB"

mkdir -p "$BASE" "$LIB"
cd "$BASE"

if [ ! -x "pg18/bin/postgres" ]; then
    echo ">> descargando PostgreSQL $PG_VERSION..."
    curl -sL -o pg.tar.gz "https://github.com/theseus-rs/postgresql-binaries/releases/download/${PG_VERSION}/postgresql-${PG_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
    tar xzf pg.tar.gz
    mv "postgresql-${PG_VERSION}-x86_64-unknown-linux-gnu" pg18
fi

if [ ! -f "$LIB/libxml2.so.2" ]; then
    echo ">> extrayendo libxml2 + libicu (deps de postgres)..."
    curl -sL -O "http://archive.ubuntu.com/ubuntu/pool/main/libx/libxml2/libxml2_2.9.10+dfsg-5_amd64.deb"
    curl -sL -O "http://archive.ubuntu.com/ubuntu/pool/main/i/icu/libicu66_66.1-2ubuntu2.1_amd64.deb"
    dpkg-deb -x libxml2_2.9.10+dfsg-5_amd64.deb libxml2-root
    dpkg-deb -x libicu66_66.1-2ubuntu2.1_amd64.deb libicu-root
    cp -a libxml2-root/usr/lib/x86_64-linux-gnu/. lib/
    cp -a libicu-root/usr/lib/x86_64-linux-gnu/. lib/
fi

if [ ! -f "$DATA/PG_VERSION" ]; then
    echo ">> inicializando cluster en $DATA..."
    ./pg18/bin/initdb -D "$DATA" -U integraciones --auth=trust >/dev/null
fi

if ! ./pg18/bin/pg_ctl -D "$DATA" status >/dev/null 2>&1; then
    echo ">> arrancando en puerto 5433..."
    ./pg18/bin/pg_ctl -D "$DATA" -o "-p 5433 -k $BASE" -l "$BASE/pg.log" start
fi

for db in integraciones integraciones_engine; do
    ./pg18/bin/createdb -h "$BASE" -p 5433 -U integraciones "$db" 2>/dev/null \
        || echo ">> db '$db' ya existe"
done

echo ">> listo. DATABASE_URL=postgres://integraciones@127.0.0.1:5433/integraciones"
