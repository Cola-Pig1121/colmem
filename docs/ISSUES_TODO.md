# Colmem Issues And TODO

This file tracks known issues, deferred fixes, and non-blocking quality gaps discovered during implementation. Add new items here when they are not important enough to interrupt the current planned milestone.

## Usage

- Record the issue, why it matters, and when it should be revisited.
- Prefer fixing blockers immediately. Defer only when the issue does not invalidate the current milestone.
- Re-check this file before starting a new implementation round.

## Active Issues

### High

- Retrieval quality is still rule-heavy and path-biased.
  - Why it matters: persisted `source_kind`, explicit ingest/rerank policies, broader golden fixtures, and the new `grounding_diagnostics` surface make the boundary clearer, but ranking is still largely hand-tuned. After fixing conversation-local LoCoMo indexing, adding generic question-stopword filtering, preserving session date context in chunks, and surfacing answerability/abstention diagnostics, the real official no-limit LoCoMo signature session run reaches Recall@1 = 0.476, Recall@5 = 0.834, and Recall@10 = 0.934 across 1982 answered evidence questions; Recall@50 is saturated because top50 covers every 19-32 retrieved session candidate pool. Feature-gated local `BAAI/bge-small-zh-v1.5` semantic embeddings improve session Recall@5 to 0.862. Dialog recall remains lower, but one-turn neighbor context plus opt-in conservative query-feature near-tie rerank improves official dialog Recall@5 to 0.573 and Recall@10 to 0.683 without using answers/evidence at retrieval time. A previous two-stage fallback that could derive gold sessions from evidence when no session candidates existed has been removed to keep benchmark semantics honest. One-pass miss-taxonomy diagnostics now separate candidate absence from rerank/order failures using actual retrieved candidate counts, and per-category diagnostics show category 1/3 dialog questions remain weak, so the next work should improve general dialog-temporal reasoning rather than tuning to data rows.
  - Suggested phase: retrieval backend/rerank hardening.

- Remote embedding fallback is implemented but not live-validated in this session.
  - Why it matters: local `BAAI/bge-small-zh-v1.5` works on this machine, and a feature-gated ModelScope/OpenAI-compatible fallback path now exists via environment variables (`COLMEM_EMBEDDING_BASE_URL`, `COLMEM_EMBEDDING_API_KEY` or `MODELSCOPE_API_KEY`, `COLMEM_EMBEDDING_MODEL`) without hardcoded tokens. It still needs real remote benchmark validation with a user-provided environment key.
  - Suggested phase: semantic embedding backend follow-up.

- Full external benchmark suites are not yet wired to the Rust `colmem` runtime.
  - Why it matters: `colmem benchmark smoke`, `colmem benchmark synthetic --size smoke`, and the Rust-native LoCoMo adapter path now run, including an official no-limit LoCoMo baseline. The older Python MemPalace benchmark suite is still a separate reference implementation, and LongMemEval/ConvoMem/MemBench coverage still needs explicit adapters and data availability checks.
  - Suggested phase: benchmark integration follow-up.

### Medium

- Top-level `hits` for fact-heavy queries can still surface development-workspace matches such as harness tests even when `Fact Evidence` is clean.
  - Why it matters: golden fixtures now cover the most direct test/doc echo cases, but the main retrieval list still reflects a self-indexed implementation workspace and may have other noisy exact matches.
  - Suggested phase: ingest policy refinement and retrieval quality follow-up.

- `facts` are persisted through workspace state, but there is no dedicated fact backend yet.
  - Why it matters: `FactStoreBackend` now defines the first backend boundary, `InMemoryFactStore` implements it, and CLI/MCP expose fact backend summary counts. A first `FactWritePolicy` layer now governs the existing CLI facts ingress with auditable `create/reinforce/supersede/invalidate/defer/reject` outcomes, but contradiction handling at larger scale and higher-volume write workflows will still outgrow the current JSON-backed store.
  - Suggested phase: Phase E.

- MCP now carries fact lifecycle semantics, argument validation, field-level output schemas, and basic notification compatibility, but the wider protocol surface is still narrow.
  - Why it matters: hosts can now rely on framed stdio, detailed `inputSchema`/`outputSchema` contracts, `ping`, empty prompts/resources/roots discovery endpoints, and `logging/setLevel`, but the wider MCP surface coverage is still incomplete.
  - Suggested phase: Phase F hardening.

- Capability permission enforcement now exists, but the policy model is still string-based and local to the harness.
  - Why it matters: `read`/`write`/`stdio` checks and stateful safety are now enforced, and permission parsing now uses a typed `CapabilityPermission` layer internally, but the serialized policy remains string-backed for compatibility and still needs broader typed reuse across runtime surfaces.
  - Suggested phase: capability enforcement hardening follow-up.

- Host rollout still lacks concrete per-host acceptance criteria.
  - Current status: dry-run install plans now include per-host templates, all-host dry-run install output, basic diagnostics, text acceptance checks, machine-readable `acceptance_plan` steps with JSON-RPC request templates including `colmem_memory_map`, static `host verify` / `host verify-all` reports, single-host `host smoke`, and all-host `host smoke-all` in-process MCP execution; each host still needs external live host proof.
  - Why it matters: “supports Claude Code/Codex/Cursor/etc.” is too soft without proving install, launch, query, and diagnostics on each host shape.
  - Suggested phase: Phase G hardening.

- Running `cargo test` and `cargo run` in parallel against the same workspace can lock the Windows incremental target directory.
  - Why it matters: the implementation workflow now requires full verification after each slice, and naive parallel cargo commands can fail with `os error 5` on `target/debug/incremental`.
  - Suggested phase: dev workflow hardening.

- The original roadmap and the Mempalace core model must both stay active.
  - Why it matters: host rollout, CLI/MCP compatibility, and later UI work should not flatten the product into generic retrieval; they must continue to expose structured spatial memory paths, facts, evidence, agent habitat, and capability decisions.
  - Suggested phase: all remaining phases.

- In this environment, fresh `cargo run` binary builds can still fail with Windows `os error 5`, even after moving past the original parallel-lock case.
  - Why it matters: unit tests pass and cover the latest MCP handler behavior, but live CLI/MCP binary verification may fall back to a stale previously-built executable unless the build-path issue is resolved.
  - Suggested phase: dev workflow hardening.

- `scripts/verify.ps1` now gives a deterministic serial verification path, but it still fails during the explicit binary refresh step because `cargo build -p colmem-cli -p colmem-mcp` can hit the same Windows `os error 5`.
  - Why it matters: this is now an explicit, reproducible blocker rather than an intermittent surprise, but live binary validation is still not reliable enough to mark the workflow hardening milestone done.
  - Suggested phase: dev workflow hardening.

- Forcing `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=1`, or isolated `CARGO_TARGET_DIR` values makes the Windows `os error 5` build problem worse in this environment.
  - Why it matters: these looked like reasonable hardening knobs, but they increase build-script cleanup and dep-graph write failures here, so future workflow work should avoid assuming they are safe.
  - Suggested phase: dev workflow hardening.

- Ingest indexes the implementation workspace itself, including tests and planning artifacts.
  - Why it matters: good for development, but it increases noise in generic queries.
  - Suggested phase: ingest policy refinement.

- Query scoring is explainable but still heuristic.
  - Why it matters: score calibration is not stable across very different query types.
  - Suggested phase: retrieval quality follow-up.

- Rerank scoring policy is now explicit in code, but not yet persisted as workspace/project configuration.
  - Why it matters: project-level source weights are now persisted, used by harness queries, and editable through CLI, but module affinity, fact-query handling, and primitive path/space scoring still cannot be tuned through CLI/state.
  - Suggested phase: retrieval calibration hardening follow-up.

- Phase F.6 policy persistence is intentionally deferred while Phase G proceeds.
  - Why it matters: `IngestPolicy` and `LightweightRerankPolicy` are explicit in code and testable, but not user-configurable through workspace/project state yet.
  - Suggested phase: retrieval policy persistence follow-up.

- Retrieval policy calibration is covered by synthetic fixtures, but not real repositories.
  - Why it matters: Phase F.6 now has meaningful golden tests for implementation/docs/tests/fact-heavy cases, but these still do not prove quality against large user corpora.
  - Suggested phase: retrieval quality follow-up.

- Memory-palace paths now have workspace/chunk persistence and a first query-time metric, but richer path diagnostics are still limited.
  - Why it matters: retrieval hits and context evidence now expose `space_path` / `memory_path`, `ContextPack` exposes a `memory_map`, CLI/MCP expose full or space-filtered memory maps, workspace state persists `memory_paths`, chunks persist `space_paths`, and hits expose `memory_path_match_count`; however, richer path diagnostics such as path-rank contribution and path mismatch explanations are not yet first-class.
  - Suggested phase: structured memory storage follow-up.

- Score saturation is mitigated with source-aware tie-breaking, but the score scale remains coarse.
  - Why it matters: multiple strong hits can still clamp to 99, making final ordering depend on tie-break logic.
  - Suggested phase: retrieval backend/rerank hardening.

- `mcp-router` is not directly exposed as a callable tool surface in the current Codex environment, even though related MCP services may be configured elsewhere on the machine.
  - Why it matters: sequential-thinking/context7/obsidian MCPs cannot currently be invoked from this rollout the same way built-in tools can, so development still needs a local-code-first fallback path.
  - Suggested phase: environment/tooling follow-up.

- The source-hygiene policy is configurable in code through `IngestPolicy`, but it is not yet persisted as workspace/project configuration.
  - Why it matters: `ProjectScope` now persists an ingest policy, default indexing uses it, and operators can inspect/update it via CLI; remaining work is broader policy UX and validation rather than the persistence hook itself.
  - Suggested phase: retrieval calibration hardening follow-up.

- PowerShell edits against files containing non-ASCII path literals can silently corrupt source encoding if the write path is not forced back to UTF-8.
  - Why it matters: this round briefly turned `ingest.rs` into invalid UTF-8, which blocks Rust compilation entirely until the file is re-encoded.
  - Suggested phase: dev workflow hardening.

- In this session, sandboxed Rust verification can fail with access-denied writes even when the same commands pass once elevated.
  - Why it matters: plain sandbox results can over-report code breakage; the reliable conclusion for this repo currently comes from elevated `cargo test` / `.\scripts\verify.ps1`.
  - Suggested phase: dev workflow hardening.

- Timed-out `cargo test` runs can leave stale `cargo`/`rustc` processes that continue holding the target directory.
  - Why it matters: follow-up verification can hang or fail with access-denied errors unless the stale verifier processes are stopped first.
  - Suggested phase: dev workflow hardening.

### Low

- Optional Vue visualization layer is planned but intentionally deferred.
  - Why it matters: observability is currently JSON/CLI-first.
  - Suggested phase: after core runtime and MCP are stable.

- Host installation automation is still dry-run only.
  - Why it matters: supported hosts now have single/all-host install/config plan output, per-host templates, standalone acceptance-check text, machine-readable acceptance plans with JSON-RPC request templates, static single/all-host verification reports, and single/all-host in-process MCP smoke checks, but setup is not yet turnkey and does not write validated host config files.
  - Suggested phase: Phase G.

## Deferred Backlog

- Configurable rerank policies instead of hardcoded Rust rules.
- Stronger semantic vector backend beyond local signature vectors.
- Conversation ingest.
- Chunk deduplication across records/projects.
- Production MCP broader protocol coverage.
- Production MCP now validates key arguments, exposes field-level `outputSchema`, supports `ping`, and ignores common notifications, but the wider MCP surface remains minimal.
- Broader MCP resource surface beyond docs-backed `resources/list` and `resources/read`.
- Golden retrieval fixtures and MCP schema snapshot tests.
- Fact source hygiene policies that distinguish implementation facts from test-only facts.
