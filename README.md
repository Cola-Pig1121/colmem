# colmem

`colmem` is a Rust-first rewrite inspired by the ideas behind the existing MemPalace repository, but it does not reuse the Python implementation or its internal algorithms. The source model, runtime boundaries, terminology, and orchestration flow are rebuilt around a host-agnostic CLI/MCP architecture.

## What Exists

- `colmem-core`: runtime types, agent profiles, evolution patches, capability registry, habitat-aware retrieval planning, context packing, and a minimal MCP server implementation.
- `colmem-cli`: a std-only CLI for bootstrapping workspaces, inspecting hosts, listing capabilities, inspecting/evolving agents, and generating query plans.
- `colmem-mcp`: a standalone stdio MCP binary backed by the core runtime.
- `colmem-hosts`: built-in host adapters for Claude Code, Codex, Cursor, Trae IDE, and OpenClaw.
- optional future `colmem-web`: a small Vue-based visualization layer for workspace, agent, retrieval, and fact inspection.

## Current Scope

This scaffold implements the agreed architecture skeleton:

- unified `Capability` management across skills, tools, plugins, and MCP endpoints
- `AgentProfile` + `AgentHabitat` as first-class runtime objects
- self-evolution patches that include persona, skills, capability preferences, and watched spaces
- `SpaceGraph` and project scope wiring for habitat-aware query planning
- `HarnessRuntimeEngine` that selects capabilities, produces a retrieval plan, builds a `ContextPack`, and previews evolution
- local workspace persistence through `.colmem/workspace-state.json`
- persisted agent evolution history shared by the CLI and MCP runtime
- project ingest with file scanning, chunking, provenance fields, and persisted local index state
- persisted local full-text index with habitat-aware scoring over indexed chunks
- local vector-signature index merged into the current hybrid retrieval path
- source-aware reranking so generic coding queries prefer implementation code over tests and planning docs
- dedicated lightweight rerank layer with module-affinity boosts for agent, retrieval, facts, context, storage, MCP, project, capability, and host code paths
- fact-aware rerank hints so active facts can lift semantically aligned evidence instead of only appearing after retrieval
- temporal/conflict-aware fact weighting so newer active facts outrank expired or superseded conflicting facts during rerank
- CLI index inspection for summary and per-chunk evidence debugging
- CLI facts listing, querying, and manual fact insertion
- fact lifecycle commands for relation replacement and explicit invalidation
- fact query/list filters for `active`, `history`, `scheduled`, and `all`
- fact audit log output for lifecycle review
- MCP fact tools for filtered fact listing, filtered fact querying, and lifecycle audit inspection
- explicit fact evidence refs using `path:`, `record:`, and `chunk:` selectors
- fact-first `ContextPack` sections that resolve supporting evidence directly from fact evidence refs before falling back to generic hits
- workspace-state normalization that upgrades legacy fact evidence ids and merges duplicate facts on load
- workspace-state normalization now backfills baseline `imported` audit events for facts that predate lifecycle tracking
- harness snapshots now serialize `fact_scope`, `fact_reference_date`, and status-aware `relevant_facts`
- MCP tool calls now include `structuredContent`, and `tools/list` exposes constrained `inputSchema` metadata with stricter argument validation

It does not yet implement a stronger semantic vector backend, conversation ingest, a dedicated high-volume fact backend, or production-grade MCP schemas and notification coverage. Those are the next layer on top of this foundation.

An optional next layer is a Vue frontend for system visualization. If added, it should stay thin and consume the same local runtime state and APIs used by the CLI/MCP surface.

## Verification

```powershell
cargo test
```

Example:

```powershell
cargo run -p colmem-cli -- init
cargo run -p colmem-cli -- ingest
cargo run -p colmem-cli -- index inspect
cargo run -p colmem-cli -- facts list active 2026-04-09
cargo run -p colmem-cli -- facts query "hybrid retrieval" active 2026-04-09
cargo run -p colmem-cli -- facts add colmem supports mcp 85 2026-04-09 - path:crates/colmem-core/src/mcp.rs
cargo run -p colmem-cli -- facts update colmem prefers "hybrid retrieval" 93 2026-04-09 path:crates/colmem-core/src/retrieval.rs
cargo run -p colmem-cli -- facts invalidate colmem supports mcp 2026-04-10
cargo run -p colmem-cli -- facts audit "colmem prefers"
cargo run -p colmem-cli -- agent evolve builder
cargo run -p colmem-cli -- query "hybrid retrieval" codex builder
'{"jsonrpc":"2.0","id":"facts","method":"tools/call","params":{"name":"colmem_fact_query","arguments":{"query":"colmem prefers retrieval","fact_scope":"active","reference_date":"2026-04-09"}}}' | cargo run -p colmem-cli -- mcp serve
```
