# EventLake Agent Guide

This file defines how AI agents must work in this repository.

The project goal is not to produce the most abstract system. The goal is to produce a Rust backend that future AI sessions can reliably understand, modify, test, and extend with limited context.

## 1. Mandatory Step Protocol

Agents must follow this order:

1. Step 1 - Architecture Design
2. Step 2 - Documentation
3. Step 3 - Context Handoff
4. Step 4 - Implementation

Before starting any step, the agent must state:

- Current step.
- What will be produced.
- Whether implementation code is allowed in that step.

Implementation code is allowed only in Step 4, and only after explicit user approval.

## 2. Current Project State

Current status:

- Architecture and documentation stage.
- No Rust implementation has been approved yet.
- No source code, migrations, Dockerfile, or docker-compose files should be created until Step 4 is explicitly requested.

Canonical documentation:

- `docs/ARCHITECTURE.md` - architecture design and module boundaries.
- `docs/SPEC.md` - product and system specification.
- `docs/BUILD.md` - build, usage, and environment guidance.
- `docs/EXTERNAL_DOCS.md` - official external documentation links.
- `docs/nextsession.md` - handoff for the next AI session.

## 3. Product Summary

EventLake is an EVM event collection, indexing, and search platform.

Core responsibilities:

- Continuously collect EVM event logs.
- Preserve raw logs permanently.
- Decode events from ABI definitions.
- Build query-optimized indexes.
- Provide one unified Search DSL.
- Provide admin and monitoring APIs.

V1 constraints:

- Language: Rust.
- Architecture: monolith first.
- Database: PostgreSQL.
- Deployment: Docker.
- Services: only `postgres` and `eventlake`.
- No Kafka, ClickHouse, S3, Elasticsearch, or Redis in V1.

## 4. Architecture Rules

All future implementation must follow these rules:

- Split modules by cognitive responsibility, not by line count.
- Each module must have one primary responsibility.
- Each file must be understandable in isolation.
- Use descriptive names. Avoid abbreviations such as `cfg`, `tmp`, `svc`, or `mgr`.
- Keep behavior explicit. Avoid hidden side effects and implicit global state.
- Prefer composition over inheritance-like patterns.
- Centralize configuration in the `configuration` module.
- Keep database access visible through module-owned repositories or query functions.
- Keep the Search DSL compiler strict and whitelist-driven.
- Store raw logs before decoding or indexing.
- Do not create duplicate active subscriptions for the same `chain_id + contract_address`.

## 5. Expected Module Boundaries

Use `docs/ARCHITECTURE.md` as the source of truth.

Primary modules:

- `app`
- `api`
- `auth`
- `configuration`
- `database`
- `chains`
- `rpc_pool`
- `abi_registry`
- `subscriptions`
- `collector`
- `reorg`
- `decoder`
- `indexing`
- `search`
- `explorers`
- `dashboard`
- `background`
- `telemetry`
- `shared`

The `shared` module must stay narrow. It may contain neutral value objects and common errors. It must not become a business-logic utility dump.

## 6. Documentation Rules

Project documentation belongs in `docs/`.

When adding integrations with external projects, update `docs/EXTERNAL_DOCS.md` with:

- Official documentation URL.
- Why the project depends on it.
- Areas of the codebase that will use it.
- Verification date.

When changing architecture, update:

- `docs/ARCHITECTURE.md`
- `docs/SPEC.md`
- `docs/nextsession.md`

When changing setup or runtime behavior, update:

- `docs/BUILD.md`
- `docs/nextsession.md`

## 7. Git Workflow

After each major step:

```text
git add .
git commit -m "feat: <describe current step>"
```

Do not push unless the user explicitly requests it.

Do not rewrite history unless the user explicitly requests it.

Do not revert user changes without explicit approval.

## 8. Self-Correction Rule

If an agent detects any of the following:

- Premature implementation before Step 4 approval.
- Poor modularization.
- Logic spreading across unrelated modules.
- A module becoming too broad to understand locally.
- Search or indexing logic becoming implicit or unsafe.

The agent must stop the current implementation path and refactor the design or documents before continuing.

## 9. Step 4 Implementation Gate

Before Step 4 starts, the agent must ask for or receive explicit approval from the user.

The first implementation phase should be incremental:

1. Create Rust project skeleton.
2. Add configuration and database connection.
3. Add migration framework.
4. Add health endpoint.
5. Add ABI registry.
6. Add chain and RPC registry.
7. Add subscription creation.
8. Add raw log storage.
9. Add collector.
10. Add decoder.
11. Add indexes.
12. Add search.
13. Add explorers and dashboard.

Each phase must be testable before moving to the next one.

