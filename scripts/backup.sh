#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# EVMEventLake Unified SQLite + ClickHouse Backup Tool
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

# Configurable defaults
BACKUP_MODE="full"       # full | incremental
BACKUP_TARGET="local"    # local | s3
BACKUP_DIR="${BACKUP_DIR:-${ROOT_DIR}/backups}"
RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-7}"

DATABASE_URL="${EVENTLAKE_DATABASE_URL:-sqlite://data/eventlake.db?mode=rwc}"
CLICKHOUSE_URL="${EVENTLAKE_CLICKHOUSE_URL:-http://eventlake:eventlake@localhost:8123/eventlake}"
CLICKHOUSE_ENABLED="${EVENTLAKE_CLICKHOUSE_ENABLED:-true}"

S3_ENDPOINT="${BACKUP_S3_ENDPOINT:-https://s3.us-east-1.amazonaws.com}"
S3_BUCKET="${BACKUP_S3_BUCKET:-}"
S3_REGION="${BACKUP_S3_REGION:-us-east-1}"
S3_PREFIX="${BACKUP_S3_PREFIX:-eventlake}"
S3_ACCESS_KEY="${BACKUP_S3_ACCESS_KEY:-}"
S3_SECRET_KEY="${BACKUP_S3_SECRET_KEY:-}"

# Parse CLI arguments
show_usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Unified SQLite + ClickHouse Backup for EVMEventLake.

Options:
  --full                Perform a full backup of SQLite and ClickHouse (default).
  --incremental         Perform an incremental backup (links to previous full/incremental backup).
  --local               Store backup locally in BACKUP_DIR (default).
  --s3                  Upload SQLite and push ClickHouse parts to S3/MinIO/R2.
  --backup-dir <path>   Specify custom local backup root directory (default: ./backups).
  -h, --help            Show this help message.

Environment Variables:
  BACKUP_DIR            Local backup storage path.
  BACKUP_S3_BUCKET      S3 bucket name.
  BACKUP_S3_ENDPOINT    S3 custom endpoint (e.g. https://<account>.r2.cloudflarestorage.com or MinIO).
  BACKUP_S3_ACCESS_KEY  S3 Access Key ID.
  BACKUP_S3_SECRET_KEY  S3 Secret Access Key.
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --full)
            BACKUP_MODE="full"
            shift
            ;;
        --incremental|--inc)
            BACKUP_MODE="incremental"
            shift
            ;;
        --local)
            BACKUP_TARGET="local"
            shift
            ;;
        --s3)
            BACKUP_TARGET="s3"
            shift
            ;;
        --backup-dir)
            BACKUP_DIR="$2"
            shift 2
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

# Resolve SQLite database file path from URL
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

TIMESTAMP="$(date -u +"%Y%m%d_%H%M%SZ")"
TAG="backup_${TIMESTAMP}_${BACKUP_MODE}"
DEST_DIR="${BACKUP_DIR}/${TAG}"
mkdir -p "${DEST_DIR}"
chmod 777 "${DEST_DIR}" 2>/dev/null || true

echo "============================================================"
echo " EVMEventLake Unified Backup Initiated"
echo " Time:       ${TIMESTAMP}"
echo " Mode:       ${BACKUP_MODE}"
echo " Target:     ${BACKUP_TARGET}"
echo " Backup Tag: ${TAG}"
echo "============================================================"

# ------------------------------------------------------------------------------
# 1. SQLite Online Atomic Snapshot (VACUUM INTO)
# ------------------------------------------------------------------------------
echo "==> [1/3] Creating SQLite operational metadata snapshot..."

if [ ! -f "${SQLITE_DB_PATH}" ]; then
    echo "Warning: SQLite database at ${SQLITE_DB_PATH} not found. Creating empty marker."
    touch "${DEST_DIR}/eventlake.db"
    SQLITE_SHA="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    SQLITE_SIZE=0
else
    TARGET_SQLITE="${DEST_DIR}/eventlake.db"
    python3 -c "
import sqlite3, sys
src = sys.argv[1]
dst = sys.argv[2]
try:
    con = sqlite3.connect(src)
    con.execute('VACUUM INTO ?', (dst,))
    con.close()
except Exception as e:
    sys.exit(f'SQLite VACUUM INTO failed: {e}')
" "${SQLITE_DB_PATH}" "${TARGET_SQLITE}"
    SQLITE_SHA="$(sha256sum "${TARGET_SQLITE}" | awk '{print $1}')"
    SQLITE_SIZE="$(stat -c%s "${TARGET_SQLITE}")"
    echo "    SQLite snapshot completed: $(du -h "${TARGET_SQLITE}" | awk '{print $1}') (SHA256: ${SQLITE_SHA:0:16}...)"
fi

# ------------------------------------------------------------------------------
# 2. ClickHouse Raw Event Lake Backup
# ------------------------------------------------------------------------------
echo "==> [2/3] Processing ClickHouse raw event lake backup..."

# Parse ClickHouse connection info
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

BASE_TAG=""
if [ "${BACKUP_MODE}" = "incremental" ]; then
    if [ -f "${BACKUP_DIR}/latest_backup.txt" ]; then
        BASE_TAG="$(cat "${BACKUP_DIR}/latest_backup.txt")"
        echo "    Incremental base detected: ${BASE_TAG}"
    else
        echo "    No previous backup found. Falling back to full ClickHouse backup."
        BACKUP_MODE="full"
    fi
fi

CH_STATUS="skipped"
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
    if [ "${BACKUP_TARGET}" = "local" ]; then
        # ClickHouse File backup (relative or absolute on server)
        CH_BACKUP_DEST="File('${DEST_DIR}/clickhouse')"
        if [ -n "${BASE_TAG}" ]; then
            BASE_BACKUP_DEST="File('${BACKUP_DIR}/${BASE_TAG}/clickhouse')"
            BACKUP_SQL="BACKUP DATABASE ${CH_DB} TO ${CH_BACKUP_DEST} SETTINGS base_backup = ${BASE_BACKUP_DEST}"
        else
            BACKUP_SQL="BACKUP DATABASE ${CH_DB} TO ${CH_BACKUP_DEST}"
        fi
    else
        # S3 ClickHouse backup
        S3_CH_PATH="${S3_ENDPOINT}/${S3_BUCKET}/${S3_PREFIX}/${TAG}/clickhouse/"
        if [ -n "${BASE_TAG}" ]; then
            S3_BASE_PATH="${S3_ENDPOINT}/${S3_BUCKET}/${S3_PREFIX}/${BASE_TAG}/clickhouse/"
            BACKUP_SQL="BACKUP DATABASE ${CH_DB} TO S3('${S3_CH_PATH}', '${S3_ACCESS_KEY}', '${S3_SECRET_KEY}') SETTINGS base_backup = S3('${S3_BASE_PATH}', '${S3_ACCESS_KEY}', '${S3_SECRET_KEY}')"
        else
            BACKUP_SQL="BACKUP DATABASE ${CH_DB} TO S3('${S3_CH_PATH}', '${S3_ACCESS_KEY}', '${S3_SECRET_KEY}')"
        fi
    fi

    # Issue SQL BACKUP command
    if [ "${EXEC_MODE}" = "direct" ]; then
        CH_RESP="$(curl -sS -u "${CH_USER}:${CH_PWD}" "${CH_SQL_URL}" --data-binary "${BACKUP_SQL}" 2>&1 || true)"
    else
        CH_RESP="$(docker exec -i "${CH_DOCKER_CID}" clickhouse-client -u "${CH_USER}" --password "${CH_PWD}" --database "${CH_DB}" --query "${BACKUP_SQL}" 2>&1 || true)"
    fi

    if echo "${CH_RESP}" | grep -qi "error"; then
        echo "    ClickHouse backup warning: ${CH_RESP}"
        CH_STATUS="warning: ${CH_RESP}"
    else
        echo "    ClickHouse backup command succeeded."
        CH_STATUS="success"
    fi
else
    echo "    ClickHouse server unreachable or disabled; raw logs backup skipped."
    CH_STATUS="offline"
fi

# ------------------------------------------------------------------------------
# 3. Write Manifest & Handle S3 Upload
# ------------------------------------------------------------------------------
echo "==> [3/3] Finalizing backup manifest..."

MANIFEST_FILE="${DEST_DIR}/manifest.json"
python3 -c "
import json, sys
data = {
  'tag': sys.argv[1],
  'timestamp': sys.argv[2],
  'mode': sys.argv[3],
  'target': sys.argv[4],
  'base_tag': sys.argv[5],
  'sqlite': {
    'file': 'eventlake.db',
    'size_bytes': int(sys.argv[6]),
    'sha256': sys.argv[7]
  },
  'clickhouse': {
    'database': sys.argv[8],
    'status': sys.argv[9]
  }
}
with open(sys.argv[10], 'w') as f:
    json.dump(data, f, indent=2)
" "${TAG}" "${TIMESTAMP}" "${BACKUP_MODE}" "${BACKUP_TARGET}" "${BASE_TAG}" "${SQLITE_SIZE}" "${SQLITE_SHA}" "${CH_DB}" "${CH_STATUS}" "${MANIFEST_FILE}"

echo "${TAG}" > "${BACKUP_DIR}/latest_backup.txt"
if [ "${BACKUP_MODE}" = "full" ]; then
    echo "${TAG}" > "${BACKUP_DIR}/latest_full_backup.txt"
fi

# Upload to S3 if requested
if [ "${BACKUP_TARGET}" = "s3" ]; then
    if [ -z "${S3_BUCKET}" ] || [ -z "${S3_ACCESS_KEY}" ] || [ -z "${S3_SECRET_KEY}" ]; then
        echo "Error: S3 configuration missing (BACKUP_S3_BUCKET, BACKUP_S3_ACCESS_KEY, BACKUP_S3_SECRET_KEY required)" >&2
        exit 1
    fi

    echo "    Uploading SQLite metadata and manifest to S3..."
    export BACKUP_S3_ENDPOINT="${S3_ENDPOINT}"
    export BACKUP_S3_BUCKET="${S3_BUCKET}"
    export BACKUP_S3_REGION="${S3_REGION}"
    export BACKUP_S3_ACCESS_KEY="${S3_ACCESS_KEY}"
    export BACKUP_S3_SECRET_KEY="${S3_SECRET_KEY}"

    python3 "${SCRIPT_DIR}/s3-helper.py" upload "${DEST_DIR}/eventlake.db" "${S3_PREFIX}/${TAG}/eventlake.db"
    python3 "${SCRIPT_DIR}/s3-helper.py" upload "${MANIFEST_FILE}" "${S3_PREFIX}/${TAG}/manifest.json"
    python3 "${SCRIPT_DIR}/s3-helper.py" upload "${BACKUP_DIR}/latest_backup.txt" "${S3_PREFIX}/latest_backup.txt"
fi

# Local retention cleanup
if [ "${BACKUP_TARGET}" = "local" ] && [ "${RETENTION_DAYS}" -gt 0 ]; then
    echo "    Pruning local backups older than ${RETENTION_DAYS} days..."
    find "${BACKUP_DIR}" -maxdepth 1 -type d -name "backup_*" -mtime +"${RETENTION_DAYS}" -exec rm -rf {} + 2>/dev/null || true
fi

echo "============================================================"
echo " Backup Completed Successfully!"
echo " Manifest:   ${MANIFEST_FILE}"
echo " Tag:        ${TAG}"
echo " Restore:    ${SCRIPT_DIR}/restore.sh --target ${TAG}"
echo "============================================================"
