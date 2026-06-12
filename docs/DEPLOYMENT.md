# EventLake Docker Deployment

Version: 1.0

Status: Draft

## 1. Deployment Modes

EventLake supports three Docker deployment entrypoints:

| Mode | Files | When to use |
| --- | --- | --- |
| Source build | `Dockerfile`, `docker-compose.yml` | CI or hosts that can download Rust crates and Debian packages. |
| Prebuilt binary | `scripts/build-prebuilt-binary.sh`, `Dockerfile.prebuilt`, `docker-compose.prebuilt.yml` | Hosts that should only package and run an already-built Linux binary. |
| China prebuilt binary | `Dockerfile.prebuilt.cn`, `docker-compose.prebuilt.cn.yml` | China mainland hosts where Docker Hub, Debian apt, or Cargo access is unstable. |

All modes keep the V1 service boundary to exactly:

- `postgres`
- `eventlake`

## 2. Runtime Facts

- Rust package: `eventlake`
- Deployed binary: `eventlake`
- Container user: `eventlake`
- HTTP host env: `EVENTLAKE_HTTP_HOST`
- HTTP port env: `EVENTLAKE_HTTP_PORT`
- Default container port: `8080`
- Default host port: `EVENTLAKE_HTTP_PORT`, default `8080`
- Health endpoint: `/health/ready`
- Persistent state: PostgreSQL only

The application compiles migrations into the binary with `sqlx::migrate!("./migrations")`.
The source-build Dockerfile copies `migrations/` into the builder stage so the release
binary includes the migration set.

## 3. Environment File

Use `.env.example` as the template for local Docker deployment. For a real deployment,
create an environment file with the same keys and change at least:

- `EVENTLAKE_JWT_SECRET`
- `EVENTLAKE_DATABASE_URL`

When using a non-default env file, pass it in both places:

```bash
EVENTLAKE_ENV_FILE=.env docker compose --env-file .env up -d --build eventlake
```

`--env-file` feeds Compose interpolation. `EVENTLAKE_ENV_FILE` selects the env file
mounted into the `eventlake` container.

## 4. Source Build Deployment

```bash
docker compose --env-file .env.example up -d --build eventlake
docker compose --env-file .env.example ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Stop services:

```bash
docker compose down
```

Remove the local PostgreSQL volume only for disposable local environments:

```bash
docker compose down -v
```

## 5. Prebuilt Binary Deployment

Build the Linux binary first:

```bash
scripts/build-prebuilt-binary.sh
```

Then build and run the lightweight runtime image:

```bash
docker compose --env-file .env.example -f docker-compose.prebuilt.yml up -d --build eventlake
docker compose --env-file .env.example -f docker-compose.prebuilt.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Optional variables:

| Variable | Purpose |
| --- | --- |
| `EVENTLAKE_PREBUILT_BINARY` | Override the generated binary path. Default: `deploy/prebuilt/eventlake`. |
| `EVENTLAKE_CARGO_TARGET` | Pass a target triple to `cargo build --target`. |
| `CARGO_TARGET_DIR` | Override Cargo's target directory. |

The prebuilt binary must match the runtime container OS, CPU architecture, and libc.
For the default Debian slim runtime, build a Linux glibc binary.

## 6. China Prebuilt Deployment

Build the binary before running Docker on the target host:

```bash
scripts/build-prebuilt-binary.sh
```

Run the China-optimized compose file:

```bash
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml up -d --build eventlake
docker compose --env-file .env.example -f docker-compose.prebuilt.cn.yml ps
curl -fsS http://127.0.0.1:8080/health/ready
```

Overridable China network variables:

| Variable | Default |
| --- | --- |
| `EVENTLAKE_DEBIAN_IMAGE` | `m.daocloud.io/docker.io/library/debian:bookworm-slim` |
| `EVENTLAKE_DEBIAN_MIRROR` | `http://mirrors.aliyun.com/debian` |
| `EVENTLAKE_DEBIAN_SECURITY_MIRROR` | `http://mirrors.aliyun.com/debian-security` |
| `EVENTLAKE_POSTGRES_IMAGE` | `m.daocloud.io/docker.io/library/postgres:18` |

## 7. Verification

Static checks:

```bash
cargo build --release --locked
scripts/build-prebuilt-binary.sh
docker compose config
docker compose -f docker-compose.prebuilt.yml config
docker compose -f docker-compose.prebuilt.cn.yml config
```

Image checks:

```bash
docker build -f Dockerfile -t eventlake:local .
docker build -f Dockerfile.prebuilt -t eventlake:prebuilt .
docker build -f Dockerfile.prebuilt.cn -t eventlake:prebuilt-cn .
```

Runtime check:

```bash
curl -fsS http://127.0.0.1:8080/health/ready
```
