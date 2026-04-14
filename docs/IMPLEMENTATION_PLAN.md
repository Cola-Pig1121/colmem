# Colmem Master Plan

This document is the canonical local plan for `colmem`. It is intended to survive chat resets and provide enough detail for implementation without relying on conversational context.

Known implementation issues and deferred fixes live in [ISSUES_TODO.md](ISSUES_TODO.md).

## 0. Execution Discipline

For each implementation round:

- check this plan before writing code
- check `ISSUES_TODO.md` and decide whether any issue is now blocking
- implement one coherent slice instead of mixing unrelated work
- run verification after code changes
- update this plan and `ISSUES_TODO.md` when the implementation state changes

## 1. Vision

`colmem` is a host-agnostic local runtime for long-term agent memory, capability orchestration, project-scoped indexing, fact constraints, and agent self-evolution.

It is inspired by the product ideas behind the original MemPalace repository, but it is not a code migration. The Rust implementation rewrites the terminology, data model, runtime boundaries, and algorithms from scratch.

The target outcome is a system that can be used from:

- Claude Code
- Codex
- Cursor
- Trae IDE
- OpenClaw
- any other tool that can invoke a CLI or connect to an MCP server

## 2. Product Goals

Primary goals:

- one local system manages memory and capabilities for many hosts
- one project attaches once and becomes available across hosts
- agents are first-class runtime objects, not stateless prompt names
- agents can evolve their persona, skill profile, capability preferences, and habitat focus
- retrieval is structure-aware and fact-aware, not only embedding-based
- the system remains explainable, local-first, and auditable

Non-goals for the first complete version:

- cloud dependency
- host-specific business logic embedded in the core runtime
- hard dependency on any single SDK
- replacing human review of major agent evolution changes

## 3. Core Principles

### 3.1 Verbatim-first

Original source material is the source of truth. Any summary, compression, ranking output, or context pack is derived data.

### 3.2 Host-agnostic runtime

The core runtime must not depend on Claude Code, Codex, Cursor, or any other single host API.

### 3.3 Unified capabilities

Skills, tools, plugins, and MCP endpoints are all modeled as `Capability`.

### 3.4 Habitat-aware agents

Agents do not query the whole world equally. Each agent has a habitat and memory priorities.

### 3.5 Controlled self-evolution

Agents may evolve persona and skills, but all changes must be logged, comparable, and reversible.

### 3.6 Retrieval before prompting

The system should avoid injecting broad system context before search. Search should stay as clean as possible, and context should be assembled after retrieval.

## 4. Terminology

The implementation should prefer engineering language over the original palace metaphor.

- `Workspace`: global runtime domain
- `ProjectScope`: one attached project and its overrides
- `Space`: semantic location or topic cluster
- `SpaceTree`: hierarchical structure of spaces
- `SpaceLink`: graph edge between spaces
- `Record`: standardized raw input item
- `Chunk`: indexable unit cut from a record
- `Fact`: structured time-aware statement
- `AgentProfile`: identity, persona, mission, role, policy, skill profile
- `AgentHabitat`: home and accessible spaces for an agent
- `Capability`: unified abstraction for skills/tools/plugins/MCP endpoints
- `CapabilityRegistry`: installed capability catalog
- `CapabilityBinding`: project or agent override for capability activation
- `ContextPack`: structured post-retrieval context bundle
- `Harness`: runtime orchestration layer
- `EvolutionCycle`: agent self-update pass

## 5. High-Level Architecture

Delivery model:

- `colmem-core`
- `colmem-cli`
- `colmem-mcp`
- `colmem-hosts`
- optional `colmem-web`

### 5.1 `colmem-core`

Owns:

- domain types
- project model
- capability registry
- agent profile and habitat
- fact store interfaces
- retrieval plan generation
- context pack generation
- harness orchestration
- evolution logic

### 5.2 `colmem-cli`

Owns:

- local operational entrypoint
- project attach/init/index commands
- diagnostics and inspection
- direct query mode
- agent inspection and evolution commands
- host install/config generation commands

### 5.3 `colmem-mcp`

Owns:

- stdio MCP server
- exposure of stable tools from core runtime
- host-neutral external protocol surface

### 5.4 `colmem-hosts`

Owns:

- host descriptors
- config/install templates
- capability compatibility mapping
- host constraints and adapter logic

### 5.5 `colmem-web` (optional)

Owns:

- local visualization UI
- workspace/project/agent inspection
- index, fact, and capability dashboards
- graph and runtime flow views
- human-friendly diagnostics on top of CLI/MCP data

This layer should stay optional. It must consume stable local interfaces from `colmem-core` or `colmem-mcp` instead of introducing its own business logic.

## 6. Data Model

### 6.1 Project and workspace

`Workspace`:

- global config root
- installed capabilities
- known projects
- known agents
- shared indexes

`ProjectScope`:

- `id`
- `name`
- `root_path`
- `tags`
- `focus_spaces`
- `required_capabilities`
- `disabled_capabilities`
- host-specific overrides

### 6.2 Spaces

`SpaceNode`:

- `id`
- `label`
- `parent_id`
- `tags`
- optional description

`SpaceLink`:

- `from`
- `to`
- `kind`
- `weight`
- optional provenance

Space responsibilities:

- hierarchy for default navigation
- graph expansion for cross-topic discovery
- candidate filtering before retrieval

### 6.3 Records and chunks

`Record` should eventually include:

- `id`
- `project_id`
- `source_type`
- `source_path`
- `created_at`
- `updated_at`
- `author`
- normalized text content
- metadata map

`Chunk` should eventually include:

- `id`
- `record_id`
- `project_id`
- `space_ids`
- text
- token estimate
- hash
- position info
- metadata

### 6.4 Facts

`Fact`:

- `subject`
- `predicate`
- `object`
- `valid_from`
- `valid_to`
- `confidence`
- `evidence_ids`

Longer-term additions:

- canonical entity ids
- aliases
- observed_at vs valid_at
- contradictory facts with ranking

### 6.5 Agents

`AgentProfile`:

- `id`
- `display_name`
- `role`
- `mission`
- `persona`
- `habitat`
- `skill_profile`
- `memory_priorities`
- `manual_capability_modes`

`PersonaProfile`:

- `voice`
- `initiative`
- `risk_appetite`
- `explanation_depth`

`SkillProfile`:

- domain weights
- preferred capabilities

`AgentHabitat`:

- `home_space`
- `accessible_spaces`
- `watch_spaces`

### 6.6 Capabilities

`CapabilityDescriptor`:

- `id`
- `kind`
- `provider`
- `version`
- `summary`
- `compatible_hosts`
- `compatible_roles`
- `project_tags`
- `permissions`
- `activation_hints`
- `stateful`

Kinds:

- `Skill`
- `Tool`
- `Plugin`
- `McpEndpoint`

## 7. Retrieval Strategy

The intended mature retrieval pipeline is:

1. identify project scope
2. identify agent profile
3. collect habitat seed spaces
4. expand candidate spaces through `SpaceTree` and `SpaceLink`
5. perform full-text retrieval
6. perform vector retrieval
7. merge candidates
8. apply fact-based boost and constraint logic
9. optionally rerank
10. build `ContextPack`

Current implementation status:

- habitat-aware candidate planning exists
- persisted local full-text index exists
- full-text retrieval against persisted chunks exists
- local vector signature index exists
- hybrid scoring now combines full-text and vector signals
- source-aware reranking now prefers implementation code over tests and planning docs for generic queries
- a dedicated lightweight rerank layer now owns source weighting and module-affinity boosts
- fact-aware rerank hints now feed relevant facts into hit ordering before context assembly
- temporal/conflict-aware fact hints now weight newer active facts above expired or superseded conflicting facts
- fact-heavy queries now classify test-like assertion chunks as test code and cap their scores below implementation hits
- harness snapshots now mark `fact_focus`, and `ContextPack` switches to fact-first presentation for strong fact queries
- harness snapshots now also carry `fact_scope`, `fact_reference_date`, and status-aware `relevant_facts`
- hits include source path and line-bound provenance
- hits now include `space_path` and `memory_path`, resolving spatial memory hierarchy from `SpaceGraph`
- `ContextPack` now includes a machine-readable `memory_map` summary with space id, readable memory path, evidence count, and top sources
- `colmem memory map [space_id]` now exposes the workspace `SpaceGraph` as a structured memory map with node paths and links, optionally filtered to one space
- workspace state now persists `memory_paths`, a normalized path index derived from `SpaceGraph`
- workspace state schema version is bumped to include persisted memory paths, and legacy loads backfill them during migration
- chunks now persist `space_paths`, so each indexed chunk carries space-specific path snapshots in addition to raw `space_ids`
- workspace load normalizes chunk `space_paths` from the current `SpaceGraph`, keeping legacy indexes compatible with the structured memory model
- query hits now expose `memory_path_match_count` as a first query-time path quality metric
- non-fact queries now keep regular retrieval evidence in `ContextPack` instead of only considering fact-evidence hits
- MCP schemas now expose `space_path`, `memory_path`, `context_pack.memory_map`, and a dedicated `colmem_memory_map` tool with optional `space_id` filtering
- backend-pluggable ranking is not implemented yet

## 8. Context Strategy

`ContextPack` should be the only structured memory payload handed to downstream agents or hosts.

Sections should eventually include:

- agent persona
- task framing
- key evidence
- key facts
- constraints
- citations
- optional action hints

Important rule:

- do not prepend a large system prompt before retrieval
- keep retrieval input clean
- add policy guidance after retrieval when building the execution context

## 9. Agent Self-Evolution

Evolution is part of the product, not an afterthought.

Allowed evolution targets:

- persona voice
- initiative
- risk appetite
- explanation depth
- skill weights
- preferred capabilities
- watched spaces
- memory priorities

Not allowed to evolve automatically:

- security model
- permission boundaries
- data retention rules
- registry truth
- project-level trust policy

`EvolutionSignal` inputs should eventually include:

- successful capability use
- failed capability use
- promoted skills
- discouraged skills
- user feedback
- contradiction frequency
- retrieval quality feedback

`EvolutionPatch` outputs:

- persona patch
- skill deltas
- capability preference additions/removals
- watched space changes
- memory priority deltas

Implementation requirements:

- every patch is recorded
- every patch can be replayed
- every patch can be reverted
- agent state can be diffed between versions

## 10. Capability Orchestration

Selection policy:

- automatic activation is primary
- manual override is secondary

Selection inputs:

- host compatibility
- agent role
- project tags
- task kind
- manual override
- project disable list
- project required list
- preferred capabilities from agent skill profile

The `Harness` should return both:

- enabled capabilities
- disabled capabilities with explicit reasons

## 11. Host Integration

Hosts currently targeted:

- Claude Code
- Codex
- Cursor
- Trae IDE
- OpenClaw

Each host adapter should define:

- host id
- display name
- transport kind
- plugin support
- compatible capability kinds
- install hint

Longer-term host adapter responsibilities:

- config file generation
- command snippets
- host-specific install output
- constraints on stateful plugins
- MCP launch wiring

## 11.5 Optional Frontend

If added, the frontend should be a small Vue application used to visualize the system rather than replace the CLI.

Recommended goals:

- inspect workspace state
- inspect attached projects
- inspect agent profiles and evolution history
- inspect capabilities and current host compatibility
- inspect indexed records/chunks with provenance
- inspect facts and evidence links
- visualize `SpaceTree` / `SpaceLink` relationships
- show query plans, hits, and `ContextPack` output

Recommended technical boundaries:

- frontend framework: Vue 3
- transport: local HTTP bridge or local MCP-backed adapter
- no direct business logic in the frontend
- frontend only renders and triggers runtime operations exposed by the backend

Suggested views:

- workspace overview
- project detail
- agent detail
- capability registry
- fact explorer
- retrieval debugger
- runtime diagnostics

## 12. Interface Documentation

### 12.1 CLI

Current implemented commands:

- `colmem init [path]`
- `colmem host list`
- `colmem host inspect <host>`
- `colmem capability list`
- `colmem project attach <name> [path]`
- `colmem project inspect`
- `colmem ingest [project_id]`
- `colmem index inspect [chunk_id]`
- `colmem facts list`
- `colmem facts query <text> [active|history|scheduled|all] [reference_date]`
- `colmem facts add <subject> <predicate> <object> [confidence] [valid_from] [valid_to]`
- `colmem facts update <subject> <predicate> <object> [confidence] [valid_from] [evidence_refs]`
- `colmem facts invalidate <subject> <predicate> [object] [valid_to]`
- `colmem facts audit [text]`
- `colmem agent inspect [id]`
- `colmem agent evolve [id]`
- `colmem query <text> [host] [agent]`
- `colmem mcp serve`

Expected future commands:

- `colmem index <project>`
- `colmem facts query <query>`
- `colmem facts add ...`
- `colmem capability bind ...`
- `colmem capability override ...`
- `colmem host install <host>`
- `colmem diagnostics`
- `colmem web serve`

### 12.2 MCP

Current minimal tool surface:

- `colmem_capability_list`
- `colmem_agent_inspect`
- `colmem_query_plan`
- `colmem_fact_list`
- `colmem_fact_query`
- `colmem_fact_audit`
- `colmem_runtime_diagnostics`

Expected future MCP tools:

- project attach/init
- search/query
- context build
- fact query/update
- capability discover/bind
- agent inspect/evolve
- diagnostics

### 12.4 Optional local HTTP/UI bridge

If the Vue frontend is added, it should not talk to Rust internals directly. Add a thin local bridge layer that exposes read-focused endpoints first.

Recommended initial endpoints:

- `GET /api/workspace`
- `GET /api/projects`
- `GET /api/projects/:id`
- `GET /api/agents`
- `GET /api/agents/:id`
- `GET /api/capabilities`
- `GET /api/index`
- `GET /api/index/chunks/:id`
- `GET /api/facts`
- `POST /api/query`

These endpoints should be derived from the same runtime state already used by CLI/MCP commands.

### 12.3 Core traits

The planned trait surface is:

- `RecordIngestor`
- `ChunkIndexer`
- `SpaceResolver`
- `Retriever`
- `FactStore`
- `ContextBuilder`
- `AgentStore`
- `CapabilityProvider`
- `CapabilityRegistryTrait`
- `BindingResolver`
- `EvolutionEngine`
- `HarnessRuntime`

These are not all fully implemented yet, but they define the intended extension boundaries.

## 13. Implementation Roadmap

### Phase A: Completed skeleton

- workspace scaffold
- host catalog
- capability registry
- project scope
- space graph
- agent profile and habitat
- evolution patch model
- retrieval planning
- context pack
- harness runtime snapshot
- CLI
- minimal MCP

### Phase B: Persistent state

Implemented:

- workspace state file at `.colmem/workspace-state.json`
- project scope persistence
- agent profile persistence
- capability registry persistence
- space graph persistence
- fact store persistence
- evolution history persistence
- CLI commands now load from persisted state
- MCP runtime now loads from persisted state

Current backend:

- local JSON via serde / serde_json

Acceptance criteria:

- restart-safe workspace state
- restart-safe agent state
- restart-safe project attachment

### Phase C: Ingest and chunking

Implemented now:

- project file ingest
- normalized record format
- chunking with stable chunk ids
- source provenance tracking with file path and line bounds
- persisted index state
- CLI index inspection

Still pending in this phase:

- conversation ingest
- deduplication across records and chunks
- richer metadata extraction
- configurable chunking policies

Acceptance criteria:

- records and chunks can be indexed from real repositories and text exports
- chunk ids are stable
- provenance can be traced back to original files

### Phase D: Retrieval backends

Implemented now:

- local full-text backend with persisted inverted index
- habitat-aware scoring on full-text matches
- local vector signature backend
- hybrid scoring across full-text and vector matches
- citations and evidence references on query hits

Still pending in this phase:

- stronger vector backend
- backend selection policy and richer reranking
- deeper quality tuning for hybrid merge behavior

Acceptance criteria:

- query returns real chunks
- ranking is explainable
- retrieval plan reflects actual backend use

### Phase E: Fact persistence and constraints

Implement:

- persistent fact store
- evidence-linked fact writes
- query-time fact constraints
- contradiction handling

Current progress:

- facts persist through workspace state
- fact-aware rerank hints exist
- temporal/conflict-aware weighting exists
- CLI fact read/write commands now exist
- fact query matching now requires meaningful token overlap instead of single-token substring matches
- strong fact queries now render `Fact Matches` and `Fact Evidence` ahead of generic evidence in `ContextPack`
- fact evidence refs now support `path:`, `record:`, and `chunk:` selectors
- `Fact Evidence` now resolves explicit evidence refs before falling back to generic aligned hits
- legacy fact evidence ids are normalized on workspace load and duplicate facts are merged
- CLI fact lifecycle commands now support relation replacement and explicit invalidation
- replacing a fact now closes older facts for the same subject/predicate relation on the effective date
- CLI fact queries and fact listings now support `active`, `history`, `scheduled`, and `all` scope filters
- fact store audit events now persist lifecycle actions for create, supersede, and invalidate flows
- MCP tools now expose fact listing, fact querying, and audit inspection with the same scope/reference-date semantics
- workspace-state normalization now backfills baseline `imported` audit events for facts that predate lifecycle tracking

Acceptance criteria:

- fact-backed constraints improve retrieval context
- facts survive restart
- evidence links are present

### Phase F: Production MCP

Implement:

- proper JSON request parsing
- argument validation
- stable tool schemas
- diagnostics and error responses

Current progress:

- MCP requests now parse through `serde_json` instead of string scanning
- numeric and string request ids are preserved in responses
- `tools/list` now returns a structured MCP result object instead of text content
- `tools/list` now exposes minimal `inputSchema` metadata for registered tools
- `tools/call` responses now include `structuredContent` alongside text content for machine-readable consumers
- MCP tools now reject missing required arguments, unsupported hosts, and malformed `reference_date` values with `-32602` invalid-params responses instead of silently defaulting
- MCP request handling now returns JSON-RPC errors for missing `method` or missing `tools/call.name` instead of dropping the session into an IO failure path
- `cargo test` covers the new MCP validation path end-to-end inside the core request handler, but direct `cargo run` validation is currently limited by a Windows `os error 5` build-path issue in this environment
- `tools/list` now exposes both `inputSchema` and `outputSchema`, giving hosts a more explicit contract for structured results
- the MCP surface now includes a basic `ping` response and ignores common notification methods such as cancelled/progress/list_changed instead of treating them as unknown requests
- MCP `outputSchema` definitions are now field-aware for capabilities, agents, query-plan hits, facts, audit events, retrieval plans, and context packs instead of only exposing top-level array/object shells
- malformed JSON now returns a parse error instead of falling through unknown-method handling
- stdio MCP transport now uses `Content-Length` framing instead of newline-delimited JSON, so it matches normal MCP client expectations
- `colmem_capability_list` now returns an object with a `capabilities` field, matching its declared `outputSchema`
- requests with missing `id` are now treated as notifications rather than receiving a fabricated response id
- `jsonrpc` envelope validation now rejects non-`2.0` requests, and static protocol methods no longer depend on workspace state loading
- empty `prompts/list`, `resources/list`, and `resources/templates/list` handlers now exist so discovery-level MCP clients can negotiate a wider protocol surface safely
- empty `roots/list` and ack-only `logging/setLevel` handlers now exist for broader MCP client compatibility
- `resources/list` and `resources/read` now expose `docs/` assets over MCP, so hosts can fetch the local plan, TODO list, and other project notes as structured resources
- `colmem-cli` command dispatch now lives in a reusable library entry, so CLI behavior can be exercised through `cargo test` without requiring a fresh `colmem-cli.exe`
- in-process CLI tests now cover `init`, `host list`, `capability list`, `facts add/query`, and an `ingest -> query` path against a temporary workspace fixture

Acceptance criteria:

- external MCP clients can call tools reliably
- malformed input is handled safely

### Phase F.5: Verification Hardening

Implement:

- deterministic serial verification workflow for Windows
- stale-binary detection guidance for CLI and MCP validation
- documented separation between logic validation (`cargo test`) and live binary validation

Current progress:

- `colmem-cli` command execution now lives in a testable library entry instead of only inside `main.rs`
- CLI tests now cover `init`, `host list`, `capability list`, `facts add/query`, and `ingest -> query`
- `scripts/verify.ps1` now treats native PowerShell command failures as real failures instead of silently continuing on non-zero exit codes
- default `scripts/verify.ps1` runs now complete successfully through logic validation (`cargo test`)
- fresh live-binary validation is still blocked by Windows `os error 5` during `cargo build`

Acceptance criteria:

- the team has one documented verification path that does not depend on accidental build-cache state
- verification notes explain when a `cargo run` result can and cannot be trusted

### Phase F.6: Retrieval Calibration And Corpus Hygiene

Implement:

- default ingest exclusions or source policies for tests, plans, and generated artifacts
- source-type weighting policy that is explicit instead of purely heuristic
- golden-query evaluation set for retrieval regressions
- retrieval regression checks for fact-heavy and implementation-heavy queries

Current progress:

- chunk `source_kind` is now persisted explicitly during ingest instead of being inferred only inside rerank
- legacy workspace loads now normalize missing/old chunk source kinds on read
- retrieval reranking now consumes persisted source kinds instead of re-deriving them from ad hoc path heuristics
- ingest regression tests now cover implementation/test/documentation/config source-kind classification
- ingest now exposes an explicit `IngestPolicy`, so corpus hygiene is configurable instead of trapped in file-walk heuristics
- project scope now persists an ingest policy, and default project indexing uses that persisted project policy instead of only the Rust default
- `colmem project ingest-policy [project_id]` now exposes the persisted project ingest policy for inspection
- `colmem project ingest-policy update <field> <add|remove> <value> [project_id]` now lets operators mutate persisted project ingest policy without hand-editing workspace JSON
- custom ingest-policy regression tests now prove that previously skipped planning/dev-note files can be re-included intentionally
- default corpus hygiene now skips planning/TODO/developer-note noise such as `IMPLEMENTATION_PLAN.md`, `ISSUES_TODO.md`, and `docs/04-开发笔记/*`
- source weighting inside the lightweight reranker now comes from an explicit policy object instead of only hardcoded branch-local constants
- project scope now persists rerank source weights, and harness query execution applies the project-level source-weight policy before reranking
- `colmem project rerank-source-weights [project_id]` and `update <field> <value> [project_id]` now let operators inspect and modify project-level source weights without editing JSON by hand
- `colmem benchmark smoke` now runs a backend-local benchmark smoke covering memory map generation, query planning, and all-host in-process MCP smoke
- `colmem benchmark synthetic --size smoke` now runs a deterministic Rust-native scoring benchmark over a synthetic corpus with source-kind, facts, and memory-path signals
- `colmem benchmark locomo --data <locomo10.json> [--limit n]` now provides a Rust-native LoCoMo dialog-retrieval adapter with structured blocked output when data is missing
- module-affinity families now come from explicit rerank policy data instead of a hardcoded in-function list
- fact-query adjustments, including the test-fixture score cap, now come from explicit rerank policy data instead of standalone branch-local rules
- low-level path-match and candidate-space bonuses now come from explicit rerank policy data through `PrimitiveScoreConfig`
- rerank regression tests now cover custom source-weight overrides in addition to the default implementation/test behavior
- rerank regression tests now also cover custom module-affinity overrides and disabling fact-query-specific penalties
- rerank regression tests now also cover custom primitive path/space scoring overrides
- golden-query regression fixtures now assert stable top hits for implementation, documentation, and test-oriented queries
- golden-query regression fixtures now also cover fact-heavy queries that must prefer implementation evidence over test/doc echoes
- golden-query regression fixtures now also cover implementation-heavy queries that must prefer runtime code over same-token documentation
- retrieval tie-breaking now uses query intent and fact presence to prefer the right source family when final scores saturate at the same value

Acceptance criteria:

- retrieval quality is measured against stable fixtures instead of manual spot checks only
- self-index noise is reduced without breaking development inspection use cases

### Phase F.7: Persistence Compatibility

Implement:

- workspace state schema versioning
- migration hooks for older persisted state
- migration tests and rollback notes

Current progress:

- workspace state now has an explicit `CURRENT_WORKSPACE_STATE_VERSION`
- saves always persist the current schema version
- loads now accept legacy state files without a `version` field
- loads now upgrade older versions to the current schema and write the migrated result back to disk
- loads reject newer unsupported schema versions with an explicit error
- regression tests now cover legacy-version upgrade, missing-version upgrade, future-version rejection, and current-version persistence

Acceptance criteria:

- older workspace state can be loaded or rejected with a clear migration path
- persistence changes are covered by regression tests

### Phase F.8: Capability Enforcement

Implement:

- explicit permission enforcement semantics for capabilities
- audit trail for capability activation decisions
- host-specific handling for unsafe or unavailable capabilities

Current progress:

- capability selection now enforces host-safe permission gates for `write`, `stdio`, and `stateful` capabilities
- capability permission parsing now uses a typed `CapabilityPermission` layer internally while preserving the existing string-backed serialized permissions field
- `ForceEnabled`, project-required, and task-requested capabilities no longer bypass permission gates or host safety checks
- harness snapshots now include capability-selection audit entries with binding mode, requirement flags, required permissions, and explainable reasons
- MCP schemas now expose `selected_capabilities.audit` for machine-readable host consumers
- regression tests now cover write-permission enforcement and stateful-capability safety enforcement

Acceptance criteria:

- automatic capability selection cannot silently cross declared permission boundaries
- capability activation decisions are explainable in logs or diagnostics

### Phase G: Host install output

Implement:

- host-specific configuration templates
- installation helpers
- compatibility diagnostics

Planning rule:

- this phase remains part of the original roadmap and is not replaced by memory-path work
- Mempalace's core model must still be preserved while implementing host rollout: every host-facing surface should be able to consume structured memory outputs rather than flattening retrieval into plain text
- structured spatial memory paths, facts, evidence, agent habitat, and capability decisions are cross-cutting requirements for Phase G and later phases

Current progress:

- host install output has started as a safe dry-run plan instead of writing host configuration files directly
- `colmem-hosts` now exposes `HostInstallPlan` with launch command, config target, config format, config snippet, diagnostics, and acceptance checks
- `HostInstallPlan` now also includes a machine-readable `acceptance_plan` with runner, action, payload, JSON-RPC request template, and expected result per smoke-check step
- `colmem host install <host> [workspace_root]` now prints a host-specific install/config plan
- `colmem host install-all [workspace_root]` now prints dry-run install/config plans for every built-in host descriptor and explicitly reports that no files are written
- `colmem host diagnostics <host> [workspace_root]` now prints host compatibility diagnostics
- `colmem host acceptance <host> [workspace_root]` now prints the smoke-check steps with payloads without requiring JSON parsing
- `colmem host verify <host> [workspace_root]` now prints a static JSON compatibility report for workspace presence, config template availability, expected tool declarations, acceptance-plan availability, and launch command declaration
- `colmem host verify-all [workspace_root]` now runs static compatibility reports across every built-in host descriptor
- `colmem host smoke <host> [workspace_root]` now runs the MCP acceptance JSON-RPC requests through the core in-process MCP handler and reports per-step pass/fail without launching an external binary
- `colmem host smoke-all [workspace_root]` now runs the in-process MCP smoke checks across every built-in host descriptor and returns an aggregate pass/fail report
- host acceptance and in-process smoke now include `colmem_memory_map`, so Phase G verifies the Mempalace core memory map surface instead of only generic query/capability tools
- host templates now cover JSON MCP server snippets, Codex TOML MCP server snippets, and CLI-plugin dry-run snippets where the host descriptor is still CLI-oriented
- `host install` now defaults to the CLI invocation workspace instead of the process current directory when no workspace root is passed
- regression tests now cover OpenClaw MCP install plan output, all-host dry-run install output, Codex TOML template output, Trae IDE diagnostics, structured acceptance smoke checks including `colmem_memory_map`, static single/all-host verify reports, single-host and all-host in-process MCP smoke execution, and default workspace-root resolution

Acceptance criteria:

- user can attach one project and reuse it across supported hosts
- per host, installation succeeds, MCP launches, tool schemas are consumable, one query works, one agent inspect works, and one capability compatibility diagnostic is available

### Phase H: Optional visualization UI

Implement:

- Vue 3 frontend scaffold
- local bridge or adapter for runtime inspection
- workspace/project/agent dashboards
- retrieval and evidence inspection views
- graph visualization for spaces and links

Acceptance criteria:

- UI can inspect the same state used by CLI/MCP
- query results and provenance are visible without reading raw JSON
- frontend remains optional and does not own core logic

### Phase E/F Follow-up: Fact backend boundary

Current progress:

- `FactStoreBackend` now defines the first backend boundary for fact add/query/scope/audit/invalidate/replace/rerank-hint operations
- `InMemoryFactStore` implements `FactStoreBackend`, preserving the current JSON-backed default while making future production backends easier to introduce
- regression tests now prove the current in-memory store satisfies the backend contract
- fact backend summary output now reports total, active, history, scheduled, inactive, and audit event counts for a reference date
- `colmem facts summary [reference_date]` and MCP `colmem_fact_summary` expose that summary for CLI and host clients

## 14. Testing Plan

### Unit tests

- capability selection logic
- agent evolution patch application
- space candidate expansion
- project override logic
- fact query matching
- context pack generation

### Integration tests

- CLI command behavior
- MCP tool behavior
- end-to-end query plan generation
- agent evolution persistence
- persisted-state migration behavior
- MCP schema contract behavior

### Future acceptance tests

- ingest real repo
- query real indexed data
- compare results across hosts
- verify manual override beats auto activation
- golden retrieval fixtures across mixed source types

## 15. Expected Milestones

Milestone 1:

- architecture skeleton compiles
- basic CLI and MCP work

Milestone 2:

- persistent workspace/projects/agents

Milestone 3:

- ingest and chunking

Milestone 4:

- real retrieval

Milestone 5:

- persistent facts and contradiction handling

Milestone 6:

- verification hardening
- retrieval calibration and corpus hygiene
- persistence compatibility guardrails

Milestone 7:

- host installation and cross-host usability

Milestone 8:

- optional local visualization UI

## 16. Current Status

Completed:

- architecture design
- crate layout
- host catalog
- capability registry
- project model
- agent model
- evolution patch model
- retrieval planning skeleton
- context pack skeleton
- harness runtime skeleton
- std-only CLI
- std-only minimal MCP
- persisted workspace state
- persisted evolution history
- real project file ingest
- persisted record/chunk index state
- lexical retrieval over indexed chunks
- line-bound provenance on query hits
- CLI index inspection
- fact matching narrowed to meaningful overlap
- MCP request handling upgraded from string matching to structured JSON parsing
- fact-query test fixtures are now explicitly recognized and capped below implementation results
- fact-focused query presentation now surfaces matched facts before supporting evidence
- explicit fact evidence selectors now resolve into dedicated `Fact Evidence` entries
- workspace state now upgrades legacy fact evidence ids and merges duplicate facts on load
- explicit fact lifecycle commands now support replacing and invalidating persisted facts
- fact scope filters and audit events now expose current-vs-historical state in the CLI
- fact scope and audit semantics now also exist on the MCP surface
- harness snapshots now expose fact lifecycle metadata directly in their serialized output
- tests for core capability selection and harness flow
- subagent review of plan realism and MCP protocol correctness
- testable CLI command runner extracted from `main.rs`
- direct `cargo test` now validates both core runtime behavior and a minimal CLI command workflow without relying on a rebuilt live binary
- `scripts/verify.ps1` now has a working default logic-validation mode and correctly fails on real PowerShell native-command errors
- workspace state persistence now has schema versioning, migration on load, and regression coverage for legacy/current/future state files
- capability selection now enforces host-safe permissions and returns an auditable decision trail in harness snapshots
- ingest now persists explicit chunk source kinds for retrieval calibration and future corpus-hygiene rules
- retrieval calibration now has explicit ingest-policy defaults, explicit source-weight policy, and golden-query regression fixtures
- host install output has started with dry-run install plans and host diagnostics
- this environment may require elevated verification runs for Rust writes, but the elevated `cargo test` and `.\scripts\verify.ps1` paths now complete successfully for the current code

Verified:

- `cargo test`
- `.\scripts\verify.ps1`
- elevated `cargo test` and elevated `.\scripts\verify.ps1` both passed after this Phase F.6 slice, confirming the current failures were environment-permission related rather than code regressions
- workspace-state migration tests for version upgrade, missing-version upgrade, future-version rejection, and current-version persistence
- framed stdio request parsing and response serialization through unit tests
- MCP schema contract regressions for capability list, notifications, JSON-RPC envelope validation, and static list endpoints
- MCP contract tests now lock the current `tools/list` tool-name order and cover `roots/list` plus `logging/setLevel`
- CLI and MCP tests now cover fact backend summary counts
- CLI benchmark smoke test covers the backend-local benchmark path
- live command smoke passed for `cargo run -p colmem-cli -- benchmark smoke`
- CLI benchmark synthetic test covers deterministic scoring output
- live command smoke passed for `cargo run -p colmem-cli -- benchmark synthetic --size smoke`
- LoCoMo tiny fixture test covers the Rust-native adapter path, including missing-data blocked output
- live command smoke passed for `cargo run -p colmem-cli -- benchmark locomo --data <tiny-fixture> --limit 1`
- live official LoCoMo run passed against `D:\Temp\locomo\data\locomo10.json` with no `--limit` using default `--granularity session`: 10 conversations, 1982 answered evidence questions, Recall@5 = 0.826, elapsed_ms = 10808
- live official LoCoMo dialog-granularity run also passed with no `--limit`: 10 conversations, 1982 answered evidence questions, Recall@5 = 0.526, elapsed_ms = 136399
- optional local semantic embedding support is feature-gated behind `semantic-embeddings`; the default local model is `BAAI/bge-small-zh-v1.5`, with `BAAI/bge-large-zh-v1.5` available through `COLMEM_LOCAL_EMBEDDING_MODEL` when hardware allows it
- remote OpenAI-compatible embedding fallback is feature-gated behind `remote-embeddings`; it reads `COLMEM_EMBEDDING_BASE_URL`, `COLMEM_EMBEDDING_API_KEY` or `MODELSCOPE_API_KEY`, and `COLMEM_EMBEDDING_MODEL`, and never hardcodes tokens
- live official LoCoMo semantic session run passed with `BAAI/bge-small-zh-v1.5`: 10 conversations, 1982 answered evidence questions, Recall@5 = 0.862, elapsed_ms = 48549
- feature checks now pass for `cargo check -p colmem-cli --features semantic-embeddings` and `cargo check -p colmem-cli --features remote-embeddings`
- LoCoMo adapter now indexes and scores each conversation independently, matching LoCoMo's conversation-local evidence ids instead of mixing repeated `D1:1`-style ids across conversations
- retrieval/fact query tokenization now filters common question stopwords, reducing lexical noise without using LoCoMo answers or evidence during retrieval
- direct CLI library tests for `init`, `host list`, `capability list`, `facts`, and `ingest -> query`
- capability-enforcement regressions for `write` permissions and stateful host safety
- capability permission parsing regression for known and unknown permission values
- ingest source-kind regressions for implementation/test/documentation/config classification
- retrieval golden queries for implementation/docs/tests and corpus-hygiene skip rules
- host install output tests for single/all-host dry-run install plan generation, per-host config templates, machine-readable acceptance plans with JSON-RPC request templates including `colmem_memory_map`, diagnostics, standalone acceptance output, static single/all-host verify output, single-host and all-host in-process MCP smoke output, and default workspace-root resolution
- structured memory-path tests for resolving `SpaceGraph` parent chains and surfacing readable memory paths in evidence context
- `ContextPack.memory_map` tests and non-fact query evidence retention tests
- CLI `memory map` tests for exposing full and space-filtered structured paths from workspace state
- MCP `colmem_memory_map` tool contract tests for exposing full and space-filtered structured memory maps to host clients
- workspace-state migration tests for backfilling missing persisted `memory_paths`
- fact backend contract test proving `InMemoryFactStore` satisfies the new `FactStoreBackend` boundary
- chunk-level `space_paths` migration is covered by the same workspace-state memory path backfill regression
- retrieval tests now assert that memory-path-heavy queries report positive `memory_path_match_count`
- ingest tests now prove persisted project ingest policy can override default corpus-hygiene skips
- CLI tests now cover inspecting persisted project ingest policy
- CLI tests now cover updating persisted project ingest policy and reloading the change
- harness tests now prove project-level rerank source weights can change result ordering
- CLI tests now cover updating persisted project rerank source weights and reloading the change
- capability permission parsing now has a typed regression test while preserving serialized string compatibility
- `cargo build -p colmem-cli -p colmem-mcp` still reproduces the current Windows live-binary blocker with `os error 5`

Not started:

- production fact backend beyond the current `FactStoreBackend` trait boundary
- robust MCP protocol layer
- host install automation beyond dry-run plan output
- optional Vue visualization layer
- richer query-time memory path metrics beyond the current `memory_path_match_count`

## 17. Immediate Next Action

The next implementation step should be:

1. continue the original Phase G host rollout plan, including dry-run install output, diagnostics, acceptance checks, and live-host proof where the environment allows it
2. keep Mempalace's core model as a non-negotiable cross-cutting requirement: structured spatial memory paths, facts, evidence, agent habitat, and capability decisions must remain visible in runtime outputs
3. keep the Windows live-binary `os error 5` build failure tracked as a separate workflow blocker in `ISSUES_TODO.md` instead of treating it as a reason to stall core implementation work

That step keeps implementation moving from runtime hardening into measurable retrieval quality work without giving up the stable validation path built in the last rounds.
