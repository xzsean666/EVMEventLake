# Prebuilt Binary

This directory is used as Docker build context input for `Dockerfile.prebuilt`
and `Dockerfile.prebuilt.cn`.

Generate the binary with:

```bash
scripts/build-prebuilt-binary.sh
```

The generated `eventlake` binary is intentionally ignored by git.

Generate the ClickHouse-enabled binary used by the ClickHouse Compose variants with:

```bash
EVENTLAKE_PREBUILT_BINARY=deploy/prebuilt/eventlake-clickhouse scripts/build-prebuilt-binary.sh
```
