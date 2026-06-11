# EventLake External Documentation Index

Version: 1.0

Verification date: 2026-06-11

Purpose:

- Give future AI sessions one place to find official external documentation.
- Avoid guessing library, protocol, chain, and deployment references.
- Keep integration links separate from product specification.

When implementation starts, re-check any dependency-specific page before pinning exact crate versions or behavior.

## 1. Rust Platform

Rust official learning and documentation entry:

- URL: https://www.rust-lang.org/learn
- Use for: Rust language, Cargo, standard library, rustdoc, compiler references.
- Relevant modules: all Rust modules.

Cargo Book:

- URL: https://doc.rust-lang.org/cargo/
- Use for: workspace structure, dependency management, build profiles, features.
- Relevant modules: project setup, build, CI.

## 2. Rust Backend Libraries

Tokio:

- URL: https://docs.rs/tokio/latest/tokio/
- Use for: async runtime, background workers, graceful shutdown, timers.
- Relevant modules: `background`, `collector`, `rpc_pool`, `app`.

Axum:

- URL: https://docs.rs/axum/latest/axum/
- Use for: HTTP routing, handlers, extractors, state sharing, middleware integration.
- Relevant modules: `api`, `auth`, `app`.

SQLx:

- URL: https://docs.rs/sqlx/latest/sqlx/
- Use for: PostgreSQL access, query typing, connection pools, transactions, migrations.
- Relevant modules: `database`, all persistence modules.
- Current implementation version: `0.9.0`.

Alloy:

- URL: https://alloy.rs/
- Use for: EVM primitives, ABI/event handling, RPC providers, Ethereum data types.
- Relevant modules: `abi_registry`, `rpc_pool`, `collector`, `decoder`.
- Current implementation crates: `alloy-primitives 1.6.0`, `alloy-json-abi 1.6.0`, `alloy-dyn-abi 1.6.0`.
- Current RPC transport note: V1 uses direct JSON-RPC HTTP calls through `reqwest` so the RPC pool can explicitly track endpoint health, latency, retries, and provider-specific failures. Alloy is used for ABI parsing, selectors, primitive EVM types, and dynamic event decoding.

Serde:

- URL: https://serde.rs/
- Use for: request/response serialization, configuration parsing, JSON fields.
- Relevant modules: `api`, `configuration`, `search`.

Utoipa:

- URL: https://docs.rs/utoipa/latest/utoipa/
- Use for: OpenAPI generation from Rust types and handlers.
- Relevant modules: `api`.
- Current implementation version: `5.5.0`.

Tracing:

- URL: https://docs.rs/tracing/latest/tracing/
- Use for: structured logs and spans.
- Relevant modules: `telemetry`, all runtime modules.

Tower:

- URL: https://docs.rs/tower/latest/tower/
- Use for: middleware and service composition with Axum.
- Relevant modules: `api`, `auth`.

## 3. Database and Deployment

PostgreSQL current documentation:

- URL: https://www.postgresql.org/docs/current/
- Use for: partitions, indexes, transactions, JSONB, query planning, performance.
- Relevant modules: `database`, `indexing`, `search`, `collector`.
- Note: On 2026-06-11 the current PostgreSQL documentation page points to PostgreSQL 18.

Docker Compose:

- URL: https://docs.docker.com/compose/
- Use for: local two-service deployment with `postgres` and `eventlake`.
- Relevant files: future `docker-compose.yml`, `Dockerfile`.

## 4. API and Auth Standards

OpenAPI Specification:

- URL: https://spec.openapis.org/oas/latest.html
- Use for: REST API schema and generated documentation semantics.
- Relevant modules: `api`.

JWT RFC 7519:

- URL: https://www.rfc-editor.org/rfc/rfc7519
- Use for: JWT structure and standard claims.
- Relevant modules: `auth`.

JWT Best Current Practices RFC 8725:

- URL: https://www.rfc-editor.org/rfc/rfc8725
- Use for: JWT validation and security hardening.
- Relevant modules: `auth`.

## 5. EVM and Ethereum Standards

Ethereum JSON-RPC API:

- URL: https://ethereum.org/developers/docs/apis/json-rpc/
- Use for: execution client JSON-RPC behavior and method semantics.
- Relevant modules: `rpc_pool`, `collector`, `reorg`.

Ethereum Execution API specification:

- URL: https://ethereum.github.io/execution-apis/
- Use for: formal JSON-RPC method definitions.
- Relevant modules: `rpc_pool`, `collector`, `reorg`.

EIP-20 ERC-20:

- URL: https://eips.ethereum.org/EIPS/eip-20
- Use for: common token `Transfer` and `Approval` event expectations.
- Relevant modules: `abi_registry`, `decoder`, `indexing`, `search`.

EIP-721 ERC-721:

- URL: https://eips.ethereum.org/EIPS/eip-721
- Use for: common NFT event expectations.
- Relevant modules: `abi_registry`, `decoder`, `indexing`, `search`.

EIP-1155 ERC-1155:

- URL: https://eips.ethereum.org/EIPS/eip-1155
- Use for: common multi-token event expectations.
- Relevant modules: `abi_registry`, `decoder`, `indexing`, `search`.

## 6. Supported Chain Documentation

Ethereum:

- URL: https://ethereum.org/developers/docs/
- Use for: Ethereum network behavior, JSON-RPC references, block/finality concepts.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.

Base:

- URL: https://docs.base.org/
- Use for: Base network, RPC, OP Stack behavior, chain-specific integration notes.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.

Arbitrum:

- URL: https://docs.arbitrum.io/
- Use for: Arbitrum network behavior, RPC, finality and L2-specific notes.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.

Optimism:

- URL: https://docs.optimism.io/
- Use for: OP Mainnet and OP Stack behavior, RPC and finality notes.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.

Polygon:

- URL: https://docs.polygon.technology/
- Use for: Polygon PoS and related network behavior.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.

BNB Smart Chain:

- URL: https://docs.bnbchain.org/bnb-smart-chain/developers/json_rpc/json-rpc-endpoint/
- Use for: BSC RPC behavior, endpoint limitations, finality notes.
- Relevant modules: `chains`, `rpc_pool`, `collector`, `reorg`.
- Note: The BNB Chain documentation states that `eth_getLogs` is disabled on listed public mainnet endpoints and recommends third-party endpoints or WebSockets for frequent log pulling. EventLake should treat BSC RPC endpoints as user-managed resources and health-check `eth_getLogs` capability.

## 7. Provider Documentation Policy

EventLake should not hard-code one RPC provider as the default provider.

If a specific provider is added later, append its official docs here before implementation.

Suggested provider categories:

- Self-hosted execution client.
- Alchemy.
- QuickNode.
- Chainstack.
- Ankr.
- NodeReal.
- Public chain RPC.

Provider-specific rate limits and log range limits must be stored as RPC endpoint metadata or chain/RPC policy, not hidden in collector code.
