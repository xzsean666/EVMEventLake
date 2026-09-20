#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# End-to-End Backup and Restore Validation Test
# ==============================================================================

TEST_DIR="$(mktemp -d -t eventlake_backup_test_XXXXXX)"
trap 'rm -rf "${TEST_DIR}"' EXIT
chmod 777 "${TEST_DIR}"

DB_FILE="${TEST_DIR}/test_eventlake.db"
BACKUP_DIR="${TEST_DIR}/backups"
mkdir -p "${BACKUP_DIR}"
chmod 777 "${BACKUP_DIR}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../scripts" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Setting up test SQLite database at ${DB_FILE}..."
# Apply baseline schema
python3 -c "
import sqlite3
con = sqlite3.connect('${DB_FILE}')
with open('${ROOT_DIR}/migrations/202609180001_initial_schema.sql') as f:
    con.executescript(f.read())
con.close()
"

export EVENTLAKE_DATABASE_URL="sqlite://${DB_FILE}?mode=rwc"
export BACKUP_DIR="${BACKUP_DIR}"
if curl -s -m 2 "http://127.0.0.1:8123/ping" | grep -q "Ok"; then
    export EVENTLAKE_CLICKHOUSE_ENABLED="true"
    export EVENTLAKE_CLICKHOUSE_URL="${EVENTLAKE_CLICKHOUSE_URL:-http://eventlake:eventlake@127.0.0.1:8123/eventlake}"
else
    export EVENTLAKE_CLICKHOUSE_ENABLED="false"
fi
export BACKUP_RETENTION_DAYS=7

echo "==> Step 1: Performing full local backup..."
"${SCRIPT_DIR}/backup.sh" --local --full

FULL_TAG="$(cat "${BACKUP_DIR}/latest_backup.txt")"
echo "    Full backup tag: ${FULL_TAG}"

echo "==> Step 2: Verifying full backup..."
"${SCRIPT_DIR}/verify-backup.sh" "${FULL_TAG}"

echo "==> Step 3: Inserting incremental test data..."
python3 -c "
import sqlite3
con = sqlite3.connect('${DB_FILE}')
con.execute(\"INSERT INTO eventlake_chains (chain_id, name, native_token_symbol) VALUES (99999, 'Custom Chain', 'CUST')\")
con.commit()
con.close()
"

echo "==> Step 4: Performing incremental local backup..."
"${SCRIPT_DIR}/backup.sh" --local --incremental

INC_TAG="$(cat "${BACKUP_DIR}/latest_backup.txt")"
echo "    Incremental backup tag: ${INC_TAG}"

echo "==> Step 5: Verifying incremental backup..."
"${SCRIPT_DIR}/verify-backup.sh" "${INC_TAG}"

echo "==> Step 6: Simulating disaster (wiping active database)..."
rm -f "${DB_FILE}"
[ ! -f "${DB_FILE}" ]

echo "==> Step 7: Restoring from latest backup (${INC_TAG})..."
"${SCRIPT_DIR}/restore.sh" --target "${INC_TAG}" --yes

echo "==> Step 8: Validating restored database..."
CHAIN_COUNT="$(python3 -c "
import sqlite3
con = sqlite3.connect('${DB_FILE}')
count = con.execute('SELECT COUNT(*) FROM eventlake_chains').fetchone()[0]
con.close()
print(count)
")"

if [ "${CHAIN_COUNT}" -ne 7 ]; then
    echo "ERROR: Expected 7 chains in restored database, got ${CHAIN_COUNT}" >&2
    exit 1
fi

echo "==> Step 9: Restoring from original full backup (${FULL_TAG})..."
"${SCRIPT_DIR}/restore.sh" --target "${FULL_TAG}" --yes

OLD_CHAIN_COUNT="$(python3 -c "
import sqlite3
con = sqlite3.connect('${DB_FILE}')
count = con.execute('SELECT COUNT(*) FROM eventlake_chains').fetchone()[0]
con.close()
print(count)
")"

if [ "${OLD_CHAIN_COUNT}" -ne 6 ]; then
    echo "ERROR: Expected 6 chains after restoring full backup, got ${OLD_CHAIN_COUNT}" >&2
    exit 1
fi

echo "============================================================"
echo " All Backup and Restore E2E Verification Tests PASSED!"
echo "============================================================"
