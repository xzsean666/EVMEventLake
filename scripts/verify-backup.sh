#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# EVMEventLake Backup Verification Tool
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

BACKUP_DIR="${BACKUP_DIR:-${ROOT_DIR}/backups}"
TARGET_TAG="${1:-}"

if [ -z "${TARGET_TAG}" ]; then
    if [ -f "${BACKUP_DIR}/latest_backup.txt" ]; then
        TARGET_TAG="$(cat "${BACKUP_DIR}/latest_backup.txt")"
    else
        echo "Usage: $0 <backup_tag_or_dir>" >&2
        exit 1
    fi
fi

if [[ "${TARGET_TAG}" == /* ]]; then
    SOURCE_DIR="${TARGET_TAG}"
else
    SOURCE_DIR="${BACKUP_DIR}/${TARGET_TAG}"
fi

MANIFEST="${SOURCE_DIR}/manifest.json"
if [ ! -f "${MANIFEST}" ]; then
    echo "Error: Manifest not found at ${MANIFEST}" >&2
    exit 1
fi

echo "============================================================"
echo " Verifying Backup: $(basename "${SOURCE_DIR}")"
echo "============================================================"

# Display Manifest Info
python3 -c "
import json
with open('${MANIFEST}') as f:
    m = json.load(f)
print(f'Tag:         {m.get(\"tag\")}')
print(f'Timestamp:   {m.get(\"timestamp\")}')
print(f'Mode:        {m.get(\"mode\")}')
print(f'Base Tag:    {m.get(\"base_tag\", \"none\")}')
print(f'ClickHouse:  {m.get(\"clickhouse\", {}).get(\"status\")}')
"

# Verify SQLite
SQLITE_FILE="${SOURCE_DIR}/eventlake.db"
if [ -f "${SQLITE_FILE}" ]; then
    echo "--> Checking SQLite snapshot..."
    EXPECTED_SHA="$(python3 -c "import json; print(json.load(open('${MANIFEST}'))['sqlite']['sha256'])")"
    ACTUAL_SHA="$(sha256sum "${SQLITE_FILE}" | awk '{print $1}')"

    if [ "${EXPECTED_SHA}" != "${ACTUAL_SHA}" ]; then
        echo "FAILED: SQLite SHA256 mismatch! Expected ${EXPECTED_SHA}, got ${ACTUAL_SHA}" >&2
        exit 1
    fi
    echo "    SHA256 Match: OK (${ACTUAL_SHA:0:16}...)"

    INTEGRITY="$(python3 -c "
import sqlite3
con = sqlite3.connect('${SQLITE_FILE}')
res = con.execute('PRAGMA integrity_check').fetchone()[0]
print(res)
tables = con.execute(\"SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'\").fetchall()
for (tbl,) in tables:
    cnt = con.execute(f'SELECT count(*) FROM {tbl}').fetchone()[0]
    print(f'    - {tbl}: {cnt} rows')
con.close()
")"
    FIRST_LINE="$(echo "${INTEGRITY}" | head -n1)"
    if [ "${FIRST_LINE}" != "ok" ]; then
        echo "FAILED: SQLite PRAGMA integrity_check returned: ${FIRST_LINE}" >&2
        exit 1
    fi
    echo "    Integrity Check: OK"
    echo "    Snapshot Table Stats:"
    echo "${INTEGRITY}" | tail -n +2
else
    echo "Notice: No SQLite database in this backup bundle."
fi

echo "============================================================"
echo " Verification PASSED!"
echo "============================================================"
