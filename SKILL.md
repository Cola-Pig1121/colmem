# colmem Agent Skill

Use this skill when an agent needs to build, test, benchmark, or integrate the `colmem` memory runtime.

## Scope and non-negotiables

- Work in this repository, not the parent Python reference benchmark: `D:\Code\Mempalace\mempalace\colmem`.
- Do not change benchmark data, labels, evidence IDs, or answers to improve scores.
- LoCoMo benchmarking must use the official dataset, typically `D:\Temp\locomo\data\locomo10.json` after cloning the official LoCoMo repository.
- Treat benchmark scores as diagnostic evidence, not the product goal. Improve retrieval, memory modeling, fusion, and ranking so the system works generally.
- Never hardcode API keys. Use environment variables for remote embedding providers.

## Common commands

```powershell
# Build/test the default local runtime
cargo test -p colmem-cli benchmark_locomo
.\scripts\verify.ps1

# Official LoCoMo session-level benchmark, signature vectors
cargo run -p colmem-cli -- benchmark locomo --data D:\Temp\locomo\data\locomo10.json --granularity session

# Official LoCoMo dialog-level benchmark, signature vectors
cargo run -p colmem-cli -- benchmark locomo --data D:\Temp\locomo\data\locomo10.json --granularity dialog

# Experimental session-to-dialog fusion
cargo run -p colmem-cli -- benchmark locomo --data D:\Temp\locomo\data\locomo10.json --granularity dialog --fusion two-stage

# Local semantic embeddings. This machine should default to BAAI/bge-small-zh-v1.5.
$env:COLMEM_LOCAL_EMBEDDING_MODEL='BAAI/bge-small-zh-v1.5'
cargo run -p colmem-cli --features semantic-embeddings -- benchmark locomo --data D:\Temp\locomo\data\locomo10.json --granularity session --embedding semantic

# Remote OpenAI-compatible embeddings through ModelScope-compatible config. Do not print or commit the key.
$env:COLMEM_EMBEDDING_BASE_URL='https://api-inference.modelscope.cn/v1'
$env:COLMEM_EMBEDDING_MODEL='Qwen/Qwen3-Embedding-8B'
$env:COLMEM_EMBEDDING_API_KEY='<set outside logs>'
cargo run -p colmem-cli --features remote-embeddings -- benchmark locomo --data D:\Temp\locomo\data\locomo10.json --granularity session --embedding remote
```

## Current benchmark interpretation

- Default signature session retrieval is the stable baseline and should stay leakage-free.
- Dialog-level retrieval is harder because evidence is turn-local; use `--dialog-window` to test small neighbor context windows without reading answers or evidence.
- `--fusion two-stage` and `--query-feature-rerank [balanced|conservative]` are experimental diagnostics. Do not enable them by default unless official no-limit runs show robust gains.
- Benchmark output includes per-category recall; inspect weak categories before tuning ranking logic.

## Rerank extension point

The core crate exposes placeholder model-rerank interface types:

- `ExternalRerankModel`
- `RerankModelRequest`
- `RerankModelCandidate`
- `RerankModelScore`

They intentionally do not call a real model yet. User integrations can implement the trait later and blend model scores with the built-in lightweight reranker. The built-in system must remain strong when no rerank model is configured; model rerank is only a bonus layer.

## Development rules

- Prefer generic improvements: better chunk boundaries, dialog context, semantic embeddings, lexical/vector fusion, and conservative reranking.
- Do not use answers or evidence to construct the retrieval index or query-time candidate set except for post-hoc metric scoring.
- After changes, run `cargo fmt`, focused tests, and a relevant official no-limit benchmark when feasible.
