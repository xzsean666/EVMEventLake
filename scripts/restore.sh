#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# EVMEventLake Unified SQLite + ClickHouse Restore Tool
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Load .env if present
if [ -f "${ROOT_DIR}/.env" ]; then
    set -a
    # shellcheck disable=SC1091
    source "${ROOT_DIR}/.env"
    set +a
fi

BACKUP_DIR="${BACKUP_DIR:-${ROOT_DIR}/backups}"
DATABASE_URL="${EVENTLAKE_DATABASE_URL:-sqlite://data/eventlake.db?mode=rwc}"
CLICKHOUSE_URL="${EVENTLAKE_CLICKHOUSE_URL:-http://eventlake:eventlake@localhost:8123/eventlake}"
CLICKHOUSE_ENABLED="${EVENTLAKE_CLICKHOUSE_ENABLED:-true}"

S3_ENDPOINT="${BACKUP_S3_ENDPOINT:-https://s3.us-east-1.amazonaws.com}"
S3_BUCKET="${BACKUP_S3_BUCKET:-}"
S3_REGION="${BACKUP_S3_REGION:-us-east-1}"
S3_PREFIX="${BACKUP_S3_PREFIX:-eventlake}"
S3_ACCESS_KEY="${BACKUP_S3_ACCESS_KEY:-}"
S3_SECRET_KEY="${BACKUP_S3_SECRET_KEY:-}"

TARGET_TAG=""
RESTORE_SOURCE="local"
ASSUME_YES=false

show_usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Unified SQLite + ClickHouse Restore Tool for EVMEventLake.

Options:
  --target <tag_or_dir> Specify backup tag (e.g. backup_20260918_120000_full) or absolute directory path.
                        If not provided, the latest backup is automatically chosen.
  --from-s3             Pull the backup from S3 before restoration.
  --local               Restore from local BACKUP_DIR (default).
  -y, --yes             Skip confirmation prompt.
  -h, --help            Show this help message.
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            TARGET_TAG="$2"
            shift 2
            ;;
        --from-s3)
            RESTORE_SOURCE="s3"
            shift
            ;;
        --local)
            RESTORE_SOURCE="local"
            shift
            ;;
        -y|--yes)
            ASSUME_YES=true
            shift
            ;;
        -h|--help)
            show_usage
            ;;
        *)
            echo "Unknown option: $1" >&2
            show_usage
            ;;
    esac
done

extract_sqlite_path() {
    local url="$1"
    local clean_path="${url#sqlite://}"
    clean_path="${clean_path%%\?*}"
    if [[ "$clean_path" != /* ]]; then
        clean_path="${ROOT_DIR}/${clean_path}"
    fi
    echo "$clean_path"
}

SQLITE_DB_PATH="$(extract_sqlite_path "${DATABASE_URL}")"

# Determine target tag
if [ -z "${TARGET_TAG}" ]; then
    if [ "${RESTORE_SOURCE}" = "s3" ]; then
        echo "Fetching latest backup tag from S3..."
        export BACKUP_S3_ENDPOINT="${S3_ENDPOINT}"
        export BACKUP_S3_BUCKET="${S3_BUCKET}"
        export BACKUP_S3_REGION="${S3_REGION}"
        export BACKUP_S3_ACCESS_KEY="${S3_ACCESS_KEY}"
        export BACKUP_S3_SECRET_KEY="${S3_SECRET_KEY}"
        python3 "${SCRIPT_DIR}/s3-helper.py" download "${BACKUP_DIR}/latest_backup.txt" "${S3_PREFIX}/latest_backup.txt"
    fi

    if [ -f "${BACKUP_DIR}/latest_backup.txt" ]; then
        TARGET_TAG="$(cat "${BACKUP_DIR}/latest_backup.txt")"
    else
        echo "Error: No backup target specified and no latest_backup.txt found in ${BACKUP_DIR}." >&2
        exit 1
    fi
fi

if [[ "${TARGET_TAG}" == /* ]]; then
    SOURCE_DIR="${TARGET_TAG}"
    TAG="$(basename "${TARGET_TAG}")"
else
    SOURCE_DIR="${BACKUP_DIR}/${TARGET_TAG}"
    TAG="${TARGET_TAG}"
fi

# Pull from S3 if requested
if [ "${RESTORE_SOURCE}" = "s3" ]; then
    echo "==> Pulling backup ${TAG} from S3..."
    mkdir -p "${SOURCE_DIR}"
    export BACKUP_S3_ENDPOINT="${S3_ENDPOINT}"
    export BACKUP_S3_BUCKET="${S3_BUCKET}"
    export BACKUP_S3_REGION="${S3_REGION}"
    export BACKUP_S3_ACCESS_KEY="${S3_ACCESS_KEY}"
    export BACKUP_S3_SECRET_KEY="${S3_SECRET_KEY}"
    python3 "${SCRIPT_DIR}/s3-helper.py" download "${SOURCE_DIR}/manifest.json" "${S3_PREFIX}/${TAG}/manifest.json"
    python3 "${SCRIPT_DIR}/s3-helper.py" download "${SOURCE_DIR}/eventlake.db" "${S3_PREFIX}/${TAG}/eventlake.db"
fi

if [ ! -f "${SOURCE_DIR}/manifest.json" ]; then
    echo "Error: Manifest not found at ${SOURCE_DIR}/manifest.json" >&2
    exit 1
fi

echo "============================================================"
echo " EVMEventLake Restore Plan"
echo " Source:       ${SOURCE_DIR}"
echo " Tag:          ${TAG}"
echo " Target DB:    ${SQLITE_DB_PATH}"
echo "============================================================"

if [ "${ASSUME_YES}" = false ]; then
    read -r -p "WARNING: Restoring will overwrite the current active SQLite and ClickHouse data. Proceed? [y/N] " confirm
    if [[ ! "${confirm}" =~ ^[yY]([eE][sS])?$ ]]; then
        echo "Restore cancelled."
        exit 0
    fi
fi

# ------------------------------------------------------------------------------
# 1. Restore SQLite Database
# ------------------------------------------------------------------------------
echo "==> [1/2] Restoring SQLite operational metadata..."

if pgrep -x "eventlake" > /dev/null 2>&1; then
    echo "Error: eventlake service is currently running. Stop the service first before restoring SQLite database to prevent WAL corruption." >&2
    exit 1
fi

mkdir -p "$(dirname "${SQLITE_DB_PATH}")"

# Backup existing database as safety fallback
if [ -f "${SQLITE_DB_PATH}" ]; then
    BAK_PATH="${SQLITE_DB_PATH}.bak.$(date +%s)"
    echo "    Backing up existing database to ${BAK_PATH}"
    cp "${SQLITE_DB_PATH}" "${BAK_PATH}"
fi

RESTORE_SQLITE="${SOURCE_DIR}/eventlake.db"
if [ -f "${RESTORE_SQLITE}" ]; then
    # Clear any residual WAL and shared memory files before overwriting
    rm -f "${SQLITE_DB_PATH}-wal" "${SQLITE_DB_PATH}-shm"
    cp "${RESTORE_SQLITE}" "${SQLITE_DB_PATH}"
    # Verify integrity
    INTEGRITY="$(python3 -c "
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
res = con.execute('PRAGMA integrity_check').fetchone()[0]
con.close()
print(res)
" "${SQLITE_DB_PATH}")"
    if [ "${INTEGRITY}" != "ok" ]; then
        echo "Error: SQLite integrity check failed: ${INTEGRITY}" >&2
        exit 1
    fi
    echo "    SQLite database restored successfully (integrity: ok)."
else
    echo "Warning: No eventlake.db in backup bundle; skipped SQLite file copy."
fi

# ------------------------------------------------------------------------------
# 2. Restore ClickHouse Raw Event Lake
# ------------------------------------------------------------------------------
echo "==> [2/2] Restoring ClickHouse raw event lake..."

parse_ch_url() {
    python3 -c "
import urllib.parse
u = urllib.parse.urlparse('${CLICKHOUSE_URL}')
host = u.hostname or 'localhost'
port = u.port or 8123
user = u.username or 'default'
pwd = u.password or ''
db = u.path.lstrip('/') or 'eventlake'
print(f'{host}:{port}:{user}:{pwd}:{db}')
"
}

CH_INFO="$(parse_ch_url)"
IFS=":" read -r CH_HOST CH_PORT CH_USER CH_PWD CH_DB <<< "${CH_INFO}"
CH_PING_URL="http://${CH_HOST}:${CH_PORT}/ping"
CH_SQL_URL="http://${CH_HOST}:${CH_PORT}/"

EXEC_MODE="none"
CH_DOCKER_CID=""

if [ "${CLICKHOUSE_ENABLED}" = "true" ]; then
    if curl -s -m 3 "${CH_PING_URL}" | grep -q "Ok"; then
        EXEC_MODE="direct"
        echo "    ClickHouse connection verified at ${CH_HOST}:${CH_PORT} (DB: ${CH_DB})"
    elif command -v docker >/dev/null 2>&1; then
        CH_DOCKER_CID="$(docker ps -q --filter "name=clickhouse" --filter "status=running" | head -n 1 || true)"
        if [ -n "${CH_DOCKER_CID}" ] && docker exec "${CH_DOCKER_CID}" wget -qO- http://127.0.0.1:8123/ping 2>/dev/null | grep -q "Ok"; then
            EXEC_MODE="docker"
            echo "    ClickHouse connection verified via Docker container (${CH_DOCKER_CID:0:12}, DB: ${CH_DB})"
        fi
    fi
fi

if [ "${EXEC_MODE}" != "none" ]; then
    if [ "${RESTORE_SOURCE}" = "local" ]; then
        CH_RESTORE_SRC="File('${SOURCE_DIR}/clickhouse')"
        RESTORE_SQL="RESTORE DATABASE ${CH_DB} FROM ${CH_RESTORE_SRC}"
    else
        S3_CH_PATH="${S3_ENDPOINT}/${S3_BUCKET}/${S3_PREFIX}/${TAG}/clickhouse/"
        RESTORE_SQL="RESTORE DATABASE ${CH_DB} FROM S3('${S3_CH_PATH}', '${S3_ACCESS_KEY}', '${S3_SECRET_KEY}')"
    fi

    if [ "${EXEC_MODE}" = "direct" ]; then
        CH_RESP="$(curl -sS -u "${CH_USER}:${CH_PWD}" "${CH_SQL_URL}" --data-binary "${RESTORE_SQL}" 2>&1 || true)"
    else
        CH_RESP="$(docker exec -i "${CH_DOCKER_CID}" clickhouse-client -u "${CH_USER}" --password "${CH_PWD}" --database "${CH_DB}" --query "${RESTORE_SQL}" 2>&1 || true)"
    fi

    if echo "${CH_RESP}" | grep -qi "error"; then
        echo "    ClickHouse restore notice: ${CH_RESP}"
    else
        echo "    ClickHouse restore command executed successfully."
    fi
else
    echo "    ClickHouse unreachable or disabled; ClickHouse restore step skipped."
fi

echo "============================================================"
echo " Restore Operation Complete!"
echo " SQLite Database:   ${SQLITE_DB_PATH}"
echo " Backup Restored:   ${TAG}"
echo "============================================================"
