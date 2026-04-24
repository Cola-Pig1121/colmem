use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use colmem_core::agent::EvolutionSignal;
use colmem_core::facts::{Fact, FactQueryScope, FactWritePolicy, InMemoryFactStore};
use colmem_core::harness::TaskIntent;
use colmem_core::host::HostContext;
use colmem_core::ingest::build_project_index;
use colmem_core::model::{HostId, TaskKind};
use colmem_core::query_feature_score;
use colmem_core::record::{
    Chunk, ChunkSourceKind, FullTextIndex, IndexState, Record, RecordSourceType, TokenPosting,
    VectorChunk, VectorIndex,
};
use colmem_core::retrieval::{HybridRetriever, QueryRequest, SearchHit};
use colmem_core::standard::standard_harness;
use colmem_core::storage::WorkspaceStateStore;
use colmem_core::utils::{json_array, json_object, quote};
use colmem_hosts::{builtin_hosts, find_host, install_plan_for_host_id};
use serde_json::Value;

pub fn run(args: Vec<String>) -> Result<String, String> {
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    run_in_dir(args, &cwd)
}

pub fn run_in_dir(args: Vec<String>, cwd: &Path) -> Result<String, String> {
    let Some(command) = args.first().cloned() else {
        return Ok(help_text());
    };

    match command.as_str() {
        "init" => cmd_init(&args[1..], cwd),
        "host" => cmd_host(&args[1..], cwd),
        "capability" => cmd_capability(&args[1..], cwd),
        "project" => cmd_project(&args[1..], cwd),
        "ingest" => cmd_ingest(&args[1..], cwd),
        "index" => cmd_index(&args[1..], cwd),
        "facts" => cmd_facts(&args[1..], cwd),
        "agent" => cmd_agent(&args[1..], cwd),
        "memory" => cmd_memory(&args[1..], cwd),
        "benchmark" => cmd_benchmark(&args[1..], cwd),
        "query" => cmd_query(&args[1..], cwd),
        "mcp" => cmd_mcp(&args[1..], cwd),
        "help" | "--help" | "-h" => Ok(help_text()),
        other => Err(format!("unknown command: {other}")),
    }
}

fn cmd_init(args: &[String], cwd: &Path) -> Result<String, String> {
    let root = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let store = WorkspaceStateStore::new(root);
    let state = store.load_or_bootstrap()?;
    Ok(format!(
        "Initialized colmem workspace at {}\nprojects={}\nagents={}",
        store.paths.state_file.display(),
        state.projects.len(),
        state.agents.len()
    ))
}

fn cmd_host(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("list") | None => Ok(builtin_hosts()
            .into_iter()
            .map(|host| {
                format!(
                    "{} [{}] {}",
                    host.display_name,
                    host.id.as_str(),
                    host.install_hint
                )
            })
            .collect::<Vec<_>>()
            .join("\n")),
        Some("inspect") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host inspect <host>".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let host = find_host(&host_id).ok_or_else(|| "host not found".to_string())?;
            Ok(host.to_json())
        }
        Some("install") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host install <host> [workspace_root]".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let workspace_root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            Ok(install_plan_for_host_id(&host_id, workspace_root)?.to_json())
        }
        Some("install-all") => {
            let workspace_root = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            Ok(host_install_all_report(workspace_root))
        }
        Some("diagnostics") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host diagnostics <host> [workspace_root]".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let workspace_root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let plan = install_plan_for_host_id(&host_id, workspace_root)?;
            Ok(plan.diagnostics.join("\n"))
        }
        Some("acceptance") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host acceptance <host> [workspace_root]".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let workspace_root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let plan = install_plan_for_host_id(&host_id, workspace_root)?;
            Ok(plan.acceptance_checks.join("\n"))
        }
        Some("verify") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host verify <host> [workspace_root]".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let workspace_root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let plan = install_plan_for_host_id(&host_id, workspace_root)?;
            Ok(host_verify_report(&plan))
        }
        Some("verify-all") => {
            let workspace_root = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            Ok(host_verify_all_report(workspace_root))
        }
        Some("smoke") => {
            let Some(raw_host) = args.get(1) else {
                return Err("usage: colmem host smoke <host> [workspace_root]".to_string());
            };
            let host_id = HostId::from_str(raw_host)?;
            let workspace_root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let plan = install_plan_for_host_id(&host_id, workspace_root)?;
            host_smoke_report(&plan)
        }
        Some("smoke-all") => {
            let workspace_root = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            host_smoke_all_report(workspace_root)
        }
        Some(other) => Err(format!("unknown host subcommand: {other}")),
    }
}

fn host_verify_check(id: &str, pass: bool, detail: String) -> String {
    json_object([
        ("id".to_string(), quote(id)),
        ("pass".to_string(), pass.to_string()),
        ("detail".to_string(), quote(&detail)),
    ])
}

fn host_install_all_report(workspace_root: String) -> String {
    let plans = builtin_hosts()
        .into_iter()
        .map(|host| colmem_hosts::install_plan_for_host(&host, workspace_root.clone()).to_json())
        .collect::<Vec<_>>();

    json_object([
        ("workspace_root".to_string(), quote(&workspace_root)),
        ("mode".to_string(), quote("dry_run_all_hosts")),
        ("writes_files".to_string(), false.to_string()),
        ("plans".to_string(), json_array(plans)),
    ])
}

fn host_verify_report(plan: &colmem_hosts::HostInstallPlan) -> String {
    let workspace_exists = Path::new(&plan.workspace_root).exists();
    let checks = vec![
        host_verify_check(
            "workspace_root_exists",
            workspace_exists,
            plan.workspace_root.clone(),
        ),
        host_verify_check(
            "config_template_available",
            !plan.config_snippet.trim().is_empty(),
            plan.config_format.clone(),
        ),
        host_verify_check(
            "expected_tools_declared",
            plan.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("expected_tools=")),
            "colmem_query_plan,colmem_agent_inspect,colmem_capability_list,colmem_memory_map"
                .to_string(),
        ),
        host_verify_check(
            "acceptance_plan_available",
            !plan.acceptance_plan.is_empty(),
            format!("steps={}", plan.acceptance_plan.len()),
        ),
        host_verify_check(
            "launch_command_declared",
            plan.command == "colmem mcp serve",
            plan.command.clone(),
        ),
    ];
    let pass = workspace_exists
        && !plan.config_snippet.trim().is_empty()
        && !plan.acceptance_plan.is_empty()
        && plan.command == "colmem mcp serve";

    json_object([
        ("host".to_string(), quote(plan.host.id.as_str())),
        ("workspace_root".to_string(), quote(&plan.workspace_root)),
        ("pass".to_string(), pass.to_string()),
        ("checks".to_string(), json_array(checks)),
    ])
}

fn host_verify_all_report(workspace_root: String) -> String {
    let reports = builtin_hosts()
        .into_iter()
        .map(|host| {
            let plan = colmem_hosts::install_plan_for_host(&host, workspace_root.clone());
            host_verify_report(&plan)
        })
        .collect::<Vec<_>>();
    let pass = reports
        .iter()
        .all(|report| report.contains("\"pass\": true"));

    json_object([
        ("workspace_root".to_string(), quote(&workspace_root)),
        ("mode".to_string(), quote("static_all_hosts")),
        ("pass".to_string(), pass.to_string()),
        ("reports".to_string(), json_array(reports)),
    ])
}

fn host_smoke_step(step: &colmem_hosts::HostAcceptanceStep, workspace_root: &str) -> String {
    if step.runner != "mcp" {
        return host_verify_check(&step.id, true, "skipped_non_mcp_step".to_string());
    }
    if step.request_json.trim().is_empty() {
        return host_verify_check(&step.id, false, "missing_request_json".to_string());
    }

    match colmem_core::mcp::handle_json_rpc_request(&step.request_json, workspace_root) {
        Ok(Some(response)) if response.contains("\"error\"") => {
            host_verify_check(&step.id, false, response)
        }
        Ok(Some(response)) => host_verify_check(&step.id, true, response),
        Ok(None) => host_verify_check(&step.id, false, "no_response".to_string()),
        Err(err) => host_verify_check(&step.id, false, err.to_string()),
    }
}

fn host_smoke_report(plan: &colmem_hosts::HostInstallPlan) -> Result<String, String> {
    let checks = plan
        .acceptance_plan
        .iter()
        .filter(|step| step.runner == "mcp")
        .map(|step| host_smoke_step(step, &plan.workspace_root))
        .collect::<Vec<_>>();
    let pass = checks.iter().all(|check| check.contains("\"pass\": true"));

    Ok(json_object([
        ("host".to_string(), quote(plan.host.id.as_str())),
        ("workspace_root".to_string(), quote(&plan.workspace_root)),
        ("mode".to_string(), quote("in_process_mcp")),
        ("pass".to_string(), pass.to_string()),
        ("checks".to_string(), json_array(checks)),
    ]))
}

fn host_smoke_all_report(workspace_root: String) -> Result<String, String> {
    let reports = builtin_hosts()
        .into_iter()
        .map(|host| {
            let plan = colmem_hosts::install_plan_for_host(&host, workspace_root.clone());
            host_smoke_report(&plan)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pass = reports
        .iter()
        .all(|report| report.contains("\"pass\": true"));

    Ok(json_object([
        ("workspace_root".to_string(), quote(&workspace_root)),
        ("mode".to_string(), quote("in_process_mcp_all_hosts")),
        ("pass".to_string(), pass.to_string()),
        ("reports".to_string(), json_array(reports)),
    ]))
}

fn cmd_capability(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("list") | None => {
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            Ok(state.registry.to_json())
        }
        Some(other) => Err(format!("unknown capability subcommand: {other}")),
    }
}

fn cmd_project(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        None => {
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            let project = state
                .primary_project()
                .ok_or_else(|| "no project in workspace".to_string())?;
            Ok(project.to_json())
        }
        Some("attach") => {
            let name = args.get(1).cloned().unwrap_or_else(|| "colmem".to_string());
            let root = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let store = WorkspaceStateStore::new(&root);
            let mut state = store.load_or_bootstrap()?;
            let mut project = state
                .primary_project()
                .cloned()
                .unwrap_or_else(|| colmem_core::standard::standard_project(root.clone()));
            project.name = name.clone();
            project.root_path = root.clone();
            state.upsert_project(project.clone());
            store.save(&state)?;
            Ok(format!(
                "Attached project '{name}' into {}\n{}",
                store.paths.state_file.display(),
                project.to_json()
            ))
        }
        Some("inspect") => {
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            let project = state
                .primary_project()
                .ok_or_else(|| "no project in workspace".to_string())?;
            Ok(project.to_json())
        }
        Some("ingest-policy") => {
            if args.get(1).map(String::as_str) == Some("update") {
                return cmd_project_ingest_policy_update(&args[2..], cwd);
            }
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            let project = args
                .get(1)
                .and_then(|project_id| state.project_by_id(project_id))
                .or_else(|| state.primary_project())
                .ok_or_else(|| "no project in workspace".to_string())?;
            Ok(project.ingest_policy.to_json())
        }
        Some("rerank-source-weights") => {
            if args.get(1).map(String::as_str) == Some("update") {
                return cmd_project_rerank_source_weights_update(&args[2..], cwd);
            }
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            let project = args
                .get(1)
                .and_then(|project_id| state.project_by_id(project_id))
                .or_else(|| state.primary_project())
                .ok_or_else(|| "no project in workspace".to_string())?;
            Ok(project.to_json())
        }
        Some(other) => Err(format!("unknown project subcommand: {other}")),
    }
}

fn cmd_project_ingest_policy_update(args: &[String], cwd: &Path) -> Result<String, String> {
    if args.len() < 3 {
        return Err(
            "usage: colmem project ingest-policy update <field> <add|remove> <value> [project_id]"
                .to_string(),
        );
    }
    let field = args[0].as_str();
    let action = args[1].as_str();
    let value = args[2].clone();
    let project_id = args.get(3).cloned();
    let store = WorkspaceStateStore::new(cwd);
    let mut state = store.load_or_bootstrap()?;
    let project = if let Some(project_id) = project_id.as_deref() {
        state
            .project_by_id_mut(project_id)
            .ok_or_else(|| format!("unknown project: {project_id}"))?
    } else {
        state
            .projects
            .first_mut()
            .ok_or_else(|| "no project in workspace".to_string())?
    };
    let add = match action {
        "add" => true,
        "remove" => false,
        other => return Err(format!("unknown ingest-policy action: {other}")),
    };

    match field {
        "skipped_dirs" => {
            if add {
                project.ingest_policy.skipped_dirs.insert(value);
            } else {
                project.ingest_policy.skipped_dirs.remove(&value);
            }
        }
        "allowed_extensions" => {
            let value = value.trim_start_matches('.').to_ascii_lowercase();
            if add {
                project.ingest_policy.allowed_extensions.insert(value);
            } else {
                project.ingest_policy.allowed_extensions.remove(&value);
            }
        }
        "skipped_file_names" => {
            if add {
                project.ingest_policy.skipped_file_names.insert(value);
            } else {
                project.ingest_policy.skipped_file_names.remove(&value);
            }
        }
        "skipped_path_fragments" => {
            if add {
                if !project
                    .ingest_policy
                    .skipped_path_fragments
                    .contains(&value)
                {
                    project.ingest_policy.skipped_path_fragments.push(value);
                }
            } else {
                project
                    .ingest_policy
                    .skipped_path_fragments
                    .retain(|fragment| fragment != &value);
            }
        }
        other => return Err(format!("unknown ingest-policy field: {other}")),
    }
    let output = project.ingest_policy.to_json();
    store.save(&state)?;
    Ok(output)
}

fn cmd_project_rerank_source_weights_update(args: &[String], cwd: &Path) -> Result<String, String> {
    if args.len() < 2 {
        return Err(
            "usage: colmem project rerank-source-weights update <field> <value> [project_id]"
                .to_string(),
        );
    }
    let field = args[0].as_str();
    let value = args[1]
        .parse::<i32>()
        .map_err(|_| format!("rerank source weight must be an integer: {}", args[1]))?;
    let project_id = args.get(2).cloned();
    let store = WorkspaceStateStore::new(cwd);
    let mut state = store.load_or_bootstrap()?;
    let project = if let Some(project_id) = project_id.as_deref() {
        state
            .project_by_id_mut(project_id)
            .ok_or_else(|| format!("unknown project: {project_id}"))?
    } else {
        state
            .projects
            .first_mut()
            .ok_or_else(|| "no project in workspace".to_string())?
    };
    match field {
        "implementation_default" => project.rerank_source_weights.implementation_default = value,
        "implementation_review" => project.rerank_source_weights.implementation_review = value,
        "implementation_refactor" => project.rerank_source_weights.implementation_refactor = value,
        "implementation_diagnose" => project.rerank_source_weights.implementation_diagnose = value,
        "test_preferred" => project.rerank_source_weights.test_preferred = value,
        "test_generic" => project.rerank_source_weights.test_generic = value,
        "documentation_preferred" => project.rerank_source_weights.documentation_preferred = value,
        "documentation_generic" => project.rerank_source_weights.documentation_generic = value,
        "config_preferred" => project.rerank_source_weights.config_preferred = value,
        "config_generic" => project.rerank_source_weights.config_generic = value,
        "plan_preferred" => project.rerank_source_weights.plan_preferred = value,
        "plan_generic" => project.rerank_source_weights.plan_generic = value,
        "generated_generic" => project.rerank_source_weights.generated_generic = value,
        other => return Err(format!("unknown rerank source weight field: {other}")),
    }
    let output = project.to_json();
    store.save(&state)?;
    Ok(output)
}

fn cmd_agent(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("inspect") | None => {
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            if let Some(id) = args.get(1) {
                let agent = state
                    .agents
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .ok_or_else(|| format!("unknown agent: {id}"))?;
                Ok(agent.to_json())
            } else {
                Ok(state
                    .agents
                    .into_iter()
                    .map(|agent| agent.to_json())
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        Some("evolve") => {
            let store = WorkspaceStateStore::new(cwd);
            let mut state = store.load_or_bootstrap()?;
            let id = args.get(1).map(String::as_str).unwrap_or("builder");
            let mut signal = EvolutionSignal::default();
            signal.promoted_skills.insert("retrieval".to_string());
            signal
                .successful_capabilities
                .insert("repo_search".to_string());
            signal.persona_shift.voice_override = Some("adaptive".to_string());
            let patch = colmem_core::agent::EvolutionPatch::from_signal(&signal);
            let output = {
                let agent = state
                    .agent_by_id_mut(id)
                    .ok_or_else(|| format!("unknown agent: {id}"))?;
                agent.apply_patch(&patch);
                agent.to_json()
            };
            state.record_evolution(id, "manual agent evolve command", signal, patch);
            store.save(&state)?;
            Ok(output)
        }
        Some(other) => Err(format!("unknown agent subcommand: {other}")),
    }
}

fn cmd_memory(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("map") | None => {
            let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
            if let Some(space_id) = args.get(1) {
                if !state.memory_paths.contains_key(space_id) {
                    return Err(format!("unknown space: {space_id}"));
                }
                return state
                    .spaces
                    .to_memory_map_json_for_space(space_id)
                    .ok_or_else(|| format!("unknown space: {space_id}"));
            }
            Ok(state.spaces.to_memory_map_json())
        }
        Some(other) => Err(format!("unknown memory subcommand: {other}")),
    }
}

fn cmd_benchmark(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("smoke") | None => benchmark_smoke(cwd),
        Some("synthetic") => benchmark_synthetic(&args[1..], cwd),
        Some("locomo") => benchmark_locomo(&args[1..], cwd),
        Some(other) => Err(format!("unknown benchmark subcommand: {other}")),
    }
}

fn benchmark_step(id: &str, elapsed_ms: u128, detail: String) -> String {
    json_object([
        ("id".to_string(), quote(id)),
        ("elapsed_ms".to_string(), elapsed_ms.to_string()),
        ("detail".to_string(), quote(&detail)),
    ])
}

fn benchmark_smoke(cwd: &Path) -> Result<String, String> {
    let total_start = Instant::now();
    let store = WorkspaceStateStore::new(cwd);
    let state = store.load_or_bootstrap()?;
    let project = state
        .primary_project()
        .cloned()
        .ok_or_else(|| "no project in workspace".to_string())?;
    let agent = state
        .agents
        .iter()
        .find(|agent| agent.id == "builder")
        .cloned()
        .ok_or_else(|| "missing builder agent".to_string())?;
    let host = find_host(&HostId::Codex).ok_or_else(|| "missing codex host".to_string())?;

    let memory_start = Instant::now();
    let memory_map = state.spaces.to_memory_map_json();
    let memory_elapsed = memory_start.elapsed().as_millis();

    let query_start = Instant::now();
    let mut harness = standard_harness();
    harness.registry = state.registry.clone();
    harness.graph = state.spaces.clone();
    harness.facts = state.facts.clone();
    harness.index = state.index.clone();
    let snapshot = harness.prepare_run(
        &agent,
        &project,
        &HostContext::new(host),
        &TaskIntent {
            kind: TaskKind::Query,
            summary: "project status memory map".to_string(),
            requested_capabilities: Default::default(),
        },
    );
    let query_elapsed = query_start.elapsed().as_millis();

    let host_start = Instant::now();
    let host_smoke = host_smoke_all_report(cwd.display().to_string())?;
    let host_elapsed = host_start.elapsed().as_millis();

    let steps = vec![
        benchmark_step(
            "memory_map",
            memory_elapsed,
            format!("bytes={}", memory_map.len()),
        ),
        benchmark_step(
            "query_plan",
            query_elapsed,
            format!(
                "hits={},memory_map={}",
                snapshot.hits.len(),
                snapshot.context_pack.memory_map.len()
            ),
        ),
        benchmark_step(
            "host_smoke_all",
            host_elapsed,
            format!("pass={}", host_smoke.contains("\"pass\": true")),
        ),
    ];

    Ok(json_object([
        ("benchmark".to_string(), quote("smoke")),
        ("pass".to_string(), true.to_string()),
        (
            "elapsed_ms".to_string(),
            total_start.elapsed().as_millis().to_string(),
        ),
        ("steps".to_string(), json_array(steps)),
    ]))
}

fn benchmark_size(args: &[String]) -> Result<String, String> {
    match args {
        [] => Ok("smoke".to_string()),
        [size] if matches!(size.as_str(), "smoke" | "small") => Ok(size.clone()),
        [flag, size] if flag == "--size" && matches!(size.as_str(), "smoke" | "small") => {
            Ok(size.clone())
        }
        _ => Err("usage: colmem benchmark synthetic [--size smoke|small]".to_string()),
    }
}

fn benchmark_tokenize(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .filter(|token| token.len() > 2)
        .map(|token| token.to_string())
        .collect()
}

fn synthetic_index(project_id: &str, graph: &colmem_core::SpaceGraph, size: &str) -> IndexState {
    let chunk_count = if size == "small" { 120 } else { 16 };
    let spaces = [
        "retrieval",
        "facts",
        "architecture",
        "agent_runtime",
        "host_adapters",
    ];
    let mut records = Vec::new();
    let mut chunks = Vec::new();
    for ordinal in 0..chunk_count {
        let space_id = spaces[ordinal % spaces.len()];
        let source_kind = match ordinal % 4 {
            0 => ChunkSourceKind::Implementation,
            1 => ChunkSourceKind::Documentation,
            2 => ChunkSourceKind::Test,
            _ => ChunkSourceKind::Config,
        };
        let source_path = match source_kind {
            ChunkSourceKind::Implementation => format!("src/synthetic_{ordinal}.rs"),
            ChunkSourceKind::Documentation => format!("docs/synthetic_{ordinal}.md"),
            ChunkSourceKind::Test => format!("tests/synthetic_{ordinal}.rs"),
            ChunkSourceKind::Config => format!("config/synthetic_{ordinal}.toml"),
            ChunkSourceKind::Plan => format!("plans/synthetic_{ordinal}.md"),
            ChunkSourceKind::Generated => format!("generated/synthetic_{ordinal}.txt"),
        };
        let needle = if ordinal == 3 {
            " SYNTHETIC_NEEDLE_003 retrieval benchmark memory map evidence"
        } else {
            ""
        };
        let text = format!(
            "synthetic colmem {space_id} chunk {ordinal} covers retrieval facts memory paths benchmark{needle}"
        );
        let record_id = format!("record-synthetic-{ordinal}");
        let chunk_id = format!("chunk-synthetic-{ordinal}");
        let space_ids = BTreeSet::from([space_id.to_string()]);
        let space_paths = BTreeMap::from([(space_id.to_string(), graph.path_labels(space_id))]);
        records.push(Record {
            id: record_id.clone(),
            project_id: project_id.to_string(),
            source_type: RecordSourceType::ProjectFile,
            source_path: source_path.clone(),
            created_at: "2026-04-13".to_string(),
            updated_at: "2026-04-13".to_string(),
            content_hash: format!("synthetic-{ordinal}"),
            content: text.clone(),
        });
        chunks.push(Chunk {
            id: chunk_id,
            record_id,
            project_id: project_id.to_string(),
            source_path,
            source_kind,
            ordinal,
            line_start: 1,
            line_end: 1,
            char_count: text.chars().count(),
            text,
            space_ids,
            space_paths,
            hash: format!("hash-synthetic-{ordinal}"),
        });
    }
    let mut postings = BTreeMap::<String, Vec<TokenPosting>>::new();
    for chunk in &chunks {
        let mut frequencies = BTreeMap::<String, u16>::new();
        for token in benchmark_tokenize(&format!("{} {}", chunk.source_path, chunk.text)) {
            *frequencies.entry(token).or_insert(0) += 1;
        }
        for (token, frequency) in frequencies {
            postings.entry(token).or_default().push(TokenPosting {
                chunk_id: chunk.id.clone(),
                frequency,
            });
        }
    }

    IndexState {
        version: 1,
        full_text: FullTextIndex {
            version: 1,
            postings,
        },
        vector: VectorIndex {
            version: 1,
            dimensions: 64,
            chunks: chunks
                .iter()
                .map(|chunk| VectorChunk {
                    chunk_id: chunk.id.clone(),
                    values: HybridRetriever::signature_vector(
                        &format!("{} {}", chunk.source_path, chunk.text),
                        64,
                    ),
                })
                .collect(),
        },
        records,
        chunks,
    }
}

fn benchmark_synthetic(args: &[String], cwd: &Path) -> Result<String, String> {
    let size = benchmark_size(args)?;
    let total_start = Instant::now();
    let store = WorkspaceStateStore::new(cwd);
    let state = store.load_or_bootstrap()?;
    let project = state
        .primary_project()
        .cloned()
        .ok_or_else(|| "no project in workspace".to_string())?;
    let agent = state
        .agents
        .iter()
        .find(|agent| agent.id == "builder")
        .cloned()
        .ok_or_else(|| "missing builder agent".to_string())?;
    let host = find_host(&HostId::Codex).ok_or_else(|| "missing codex host".to_string())?;
    let index = synthetic_index(&project.id, &state.spaces, &size);

    let query_start = Instant::now();
    let mut harness = standard_harness();
    harness.registry = state.registry.clone();
    harness.graph = state.spaces.clone();
    harness.facts = state.facts.clone();
    harness.index = index.clone();
    let snapshot = harness.prepare_run(
        &agent,
        &project,
        &HostContext::new(host),
        &TaskIntent {
            kind: TaskKind::Query,
            summary: "SYNTHETIC_NEEDLE_003 retrieval benchmark memory map evidence".to_string(),
            requested_capabilities: Default::default(),
        },
    );
    let query_elapsed = query_start.elapsed().as_millis();
    let top_hit_contains_needle = snapshot
        .hits
        .first()
        .is_some_and(|hit| hit.snippet.contains("SYNTHETIC_NEEDLE_003"));
    let path_match_total = snapshot
        .hits
        .iter()
        .map(|hit| hit.memory_path_match_count)
        .sum::<usize>();
    let pass = top_hit_contains_needle && !snapshot.context_pack.memory_map.is_empty();

    Ok(json_object([
        ("benchmark".to_string(), quote("synthetic")),
        ("dataset".to_string(), quote("synthetic")),
        ("mode".to_string(), quote("query_plan")),
        ("size".to_string(), quote(&size)),
        ("pass".to_string(), pass.to_string()),
        (
            "elapsed_ms".to_string(),
            total_start.elapsed().as_millis().to_string(),
        ),
        (
            "metrics".to_string(),
            json_object([
                ("chunks".to_string(), index.chunks.len().to_string()),
                ("records".to_string(), index.records.len().to_string()),
                ("hits".to_string(), snapshot.hits.len().to_string()),
                (
                    "top_hit_contains_needle".to_string(),
                    top_hit_contains_needle.to_string(),
                ),
                (
                    "context_memory_map_entries".to_string(),
                    snapshot.context_pack.memory_map.len().to_string(),
                ),
                (
                    "memory_path_match_total".to_string(),
                    path_match_total.to_string(),
                ),
                ("query_elapsed_ms".to_string(), query_elapsed.to_string()),
            ]),
        ),
        (
            "thresholds".to_string(),
            json_object([
                ("top_hit_contains_needle".to_string(), true.to_string()),
                (
                    "context_memory_map_entries_min".to_string(),
                    "1".to_string(),
                ),
            ]),
        ),
    ]))
}

fn benchmark_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn first_evidence_rank(hits: &[SearchHit], evidence: &BTreeSet<String>) -> Option<usize> {
    hits.iter()
        .position(|hit| evidence.contains(&hit.chunk_id))
        .map(|index| index + 1)
}

fn locomo_evidence_ids(evidence: &BTreeSet<String>, granularity: &str) -> BTreeSet<String> {
    if granularity == "dialog" {
        return evidence.clone();
    }
    evidence
        .iter()
        .filter_map(|id| {
            let rest = id.strip_prefix('D')?;
            let (session, _) = rest.split_once(':')?;
            Some(format!("session_{session}"))
        })
        .collect()
}

fn locomo_dialog_line(dialog: &Value) -> String {
    let speaker = dialog
        .get("speaker")
        .and_then(Value::as_str)
        .unwrap_or("speaker");
    let text = dialog
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{speaker} said, \"{text}\"")
}

fn locomo_dialog_context(
    dialogs: &[Value],
    dialog_index: usize,
    date: &str,
    window: usize,
) -> String {
    let start = dialog_index.saturating_sub(window);
    let end = (dialog_index + window + 1).min(dialogs.len());
    let mut lines = Vec::new();
    if !date.is_empty() {
        lines.push(format!("Session date: {date}."));
    }
    for (index, dialog) in dialogs.iter().enumerate().take(end).skip(start) {
        let role = if index < dialog_index {
            "Previous"
        } else if index == dialog_index {
            "Current"
        } else {
            "Next"
        };
        lines.push(format!("{role}: {}", locomo_dialog_line(dialog)));
    }
    lines.join("\n")
}

fn locomo_rerank_hits_by_query_features(
    question: &str,
    hits: &mut [colmem_core::retrieval::SearchHit],
    mode: &str,
) {
    if mode == "conservative" {
        let mut keyed = hits
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, mut hit)| {
                let feature_score =
                    query_feature_score(question, &hit.source_path, &hit.snippet).weighted_total();
                if feature_score > 0 {
                    hit.reasons
                        .push(format!("query-feature near-tie signal={feature_score}"));
                }
                (index, feature_score, hit)
            })
            .collect::<Vec<_>>();
        keyed.sort_by(|left, right| {
            let left_score = left.2.score;
            let right_score = right.2.score;
            let left_band = left_score / 4;
            let right_band = right_score / 4;
            right_band
                .cmp(&left_band)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| right_score.cmp(&left_score))
                .then_with(|| left.2.source_path.cmp(&right.2.source_path))
                .then_with(|| left.2.chunk_id.cmp(&right.2.chunk_id))
                .then_with(|| left.0.cmp(&right.0))
        });
        for (slot, (_, _, hit)) in hits.iter_mut().zip(keyed.into_iter()) {
            *slot = hit;
        }
        return;
    }

    for hit in hits.iter_mut() {
        let boost = query_feature_score(question, &hit.source_path, &hit.snippet).weighted_total();
        if boost > 0 {
            hit.score = hit.score.saturating_add(boost.min(30) as u8).min(99);
            hit.reasons.push(format!("query-feature boost={boost}"));
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
}

fn vector_index_for_chunks(chunks: &[Chunk], embedding: &str) -> Result<VectorIndex, String> {
    if embedding == "semantic" || embedding == "remote" {
        let texts = chunks
            .iter()
            .map(|chunk| format!("{} {}", chunk.source_path, chunk.text))
            .collect::<Vec<_>>();
        let vectors = if embedding == "semantic" {
            semantic_embed_texts(&texts)?
        } else {
            remote_embed_texts(&texts)?
        };
        let dimensions = vectors.first().map(Vec::len).unwrap_or(0);
        return Ok(VectorIndex {
            version: 1,
            dimensions,
            chunks: chunks
                .iter()
                .zip(vectors)
                .map(|(chunk, values)| VectorChunk {
                    chunk_id: chunk.id.clone(),
                    values,
                })
                .collect(),
        });
    }

    Ok(VectorIndex {
        version: 1,
        dimensions: 64,
        chunks: chunks
            .iter()
            .map(|chunk| VectorChunk {
                chunk_id: chunk.id.clone(),
                values: HybridRetriever::signature_vector(
                    &format!("{} {}", chunk.source_path, chunk.text),
                    64,
                ),
            })
            .collect(),
    })
}

fn semantic_embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    #[cfg(feature = "semantic-embeddings")]
    {
        colmem_core::semantic::embed_texts(texts)
    }
    #[cfg(not(feature = "semantic-embeddings"))]
    {
        let _ = texts;
        Err("semantic embeddings feature is disabled".to_string())
    }
}

fn remote_embed_texts(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    #[cfg(feature = "remote-embeddings")]
    {
        let api_key = env::var("COLMEM_EMBEDDING_API_KEY")
            .or_else(|_| env::var("MODELSCOPE_API_KEY"))
            .map_err(|_| "remote embedding API key missing: set COLMEM_EMBEDDING_API_KEY or MODELSCOPE_API_KEY".to_string())?;
        let base_url = env::var("COLMEM_EMBEDDING_BASE_URL")
            .unwrap_or_else(|_| "https://api-inference.modelscope.cn/v1".to_string());
        let model = env::var("COLMEM_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "Qwen/Qwen3-Embedding-8B".to_string());
        let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
        let response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "model": model,
                "input": texts,
                "encoding_format": "float"
            }))
            .map_err(|err| err.to_string())?;
        let value = response
            .into_json::<Value>()
            .map_err(|err| err.to_string())?;
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "remote embedding response missing data[]".to_string())?;
        data.iter()
            .map(|item| {
                item.get("embedding")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "remote embedding item missing embedding[]".to_string())?
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .map(|number| number as f32)
                            .ok_or_else(|| "remote embedding value was not a number".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect()
    }
    #[cfg(not(feature = "remote-embeddings"))]
    {
        let _ = texts;
        Err("remote embeddings feature is disabled".to_string())
    }
}

fn locomo_index_for_sample(
    sample_index: usize,
    sample: &Value,
    project_id: &str,
    graph: &colmem_core::SpaceGraph,
    granularity: &str,
    embedding: &str,
    dialog_window: usize,
) -> Result<IndexState, String> {
    let mut records = Vec::new();
    let mut chunks = Vec::new();
    if let Some(conversation) = sample.get("conversation") {
        let mut session_number = 1usize;
        loop {
            let key = format!("session_{session_number}");
            let Some(dialogs) = conversation.get(&key).and_then(Value::as_array) else {
                break;
            };
            let date = conversation
                .get(format!("session_{session_number}_date_time"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if granularity == "session" {
                let content = dialogs
                    .iter()
                    .map(|dialog| {
                        let speaker = dialog
                            .get("speaker")
                            .and_then(Value::as_str)
                            .unwrap_or("speaker");
                        let text = dialog
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        format!("{speaker} said, \"{text}\"")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let content = if date.is_empty() {
                    content
                } else {
                    format!("Session date: {date}\n{content}")
                };
                let source_path =
                    format!("locomo/sample_{sample_index}/session_{session_number}.txt");
                let space_id = "facts".to_string();
                let record_id = format!("record-locomo-{sample_index}-session-{session_number}");
                records.push(Record {
                    id: record_id.clone(),
                    project_id: project_id.to_string(),
                    source_type: RecordSourceType::ConversationExport,
                    source_path: source_path.clone(),
                    created_at: "locomo".to_string(),
                    updated_at: "locomo".to_string(),
                    content_hash: format!("locomo-{sample_index}-session-{session_number}"),
                    content: content.clone(),
                });
                chunks.push(Chunk {
                    id: format!("session_{session_number}"),
                    record_id,
                    project_id: project_id.to_string(),
                    source_path,
                    source_kind: ChunkSourceKind::Documentation,
                    ordinal: session_number,
                    line_start: 1,
                    line_end: dialogs.len().max(1),
                    char_count: content.chars().count(),
                    text: content,
                    space_ids: BTreeSet::from([space_id.clone()]),
                    space_paths: BTreeMap::from([(space_id.clone(), graph.path_labels(&space_id))]),
                    hash: format!("hash-locomo-{sample_index}-session-{session_number}"),
                });
                session_number += 1;
                continue;
            }
            for (dialog_index, dialog) in dialogs.iter().enumerate() {
                let dia_id = dialog
                    .get("dia_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("D{session_number}:{}", dialog_index + 1));
                let content = locomo_dialog_context(dialogs, dialog_index, date, dialog_window);
                let source_path = format!("locomo/sample_{sample_index}/{dia_id}.txt");
                let space_id = "facts".to_string();
                let record_id = format!("record-locomo-{sample_index}-{dia_id}");
                records.push(Record {
                    id: record_id.clone(),
                    project_id: project_id.to_string(),
                    source_type: RecordSourceType::ConversationExport,
                    source_path: source_path.clone(),
                    created_at: "locomo".to_string(),
                    updated_at: "locomo".to_string(),
                    content_hash: format!("locomo-{sample_index}-{dia_id}"),
                    content: content.clone(),
                });
                chunks.push(Chunk {
                    id: dia_id,
                    record_id,
                    project_id: project_id.to_string(),
                    source_path,
                    source_kind: ChunkSourceKind::Documentation,
                    ordinal: dialog_index,
                    line_start: 1,
                    line_end: 1,
                    char_count: content.chars().count(),
                    text: content,
                    space_ids: BTreeSet::from([space_id.clone()]),
                    space_paths: BTreeMap::from([(space_id.clone(), graph.path_labels(&space_id))]),
                    hash: format!("hash-locomo-{sample_index}-{session_number}-{dialog_index}"),
                });
            }
            session_number += 1;
        }
    }

    let mut postings = BTreeMap::<String, Vec<TokenPosting>>::new();
    for chunk in &chunks {
        let mut frequencies = BTreeMap::<String, u16>::new();
        for token in benchmark_tokenize(&format!("{} {}", chunk.source_path, chunk.text)) {
            *frequencies.entry(token).or_insert(0) += 1;
        }
        for (token, frequency) in frequencies {
            postings.entry(token).or_default().push(TokenPosting {
                chunk_id: chunk.id.clone(),
                frequency,
            });
        }
    }

    let vector = vector_index_for_chunks(&chunks, embedding)?;

    Ok(IndexState {
        version: 1,
        full_text: FullTextIndex {
            version: 1,
            postings,
        },
        vector,
        records,
        chunks,
    })
}

fn locomo_restricted_dialog_index_for_sample(
    sample_index: usize,
    sample: &Value,
    project_id: &str,
    graph: &colmem_core::SpaceGraph,
    session_ids: &BTreeSet<String>,
    embedding: &str,
    dialog_window: usize,
) -> Result<IndexState, String> {
    let mut records = Vec::new();
    let mut chunks = Vec::new();
    if let Some(conversation) = sample.get("conversation") {
        let mut session_number = 1usize;
        loop {
            let session_id = format!("session_{session_number}");
            let key = format!("session_{session_number}");
            let Some(dialogs) = conversation.get(&key).and_then(Value::as_array) else {
                break;
            };
            if !session_ids.contains(&session_id) {
                session_number += 1;
                continue;
            }
            let date = conversation
                .get(format!("session_{session_number}_date_time"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            for (dialog_index, dialog) in dialogs.iter().enumerate() {
                let dia_id = dialog
                    .get("dia_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("D{session_number}:{}", dialog_index + 1));
                let content = locomo_dialog_context(dialogs, dialog_index, date, dialog_window);
                let source_path = format!("locomo/sample_{sample_index}/{dia_id}.txt");
                let space_id = "facts".to_string();
                let record_id = format!("record-locomo-{sample_index}-{dia_id}");
                records.push(Record {
                    id: record_id.clone(),
                    project_id: project_id.to_string(),
                    source_type: RecordSourceType::ConversationExport,
                    source_path: source_path.clone(),
                    created_at: "locomo".to_string(),
                    updated_at: "locomo".to_string(),
                    content_hash: format!("locomo-{sample_index}-{dia_id}"),
                    content: content.clone(),
                });
                chunks.push(Chunk {
                    id: dia_id,
                    record_id,
                    project_id: project_id.to_string(),
                    source_path,
                    source_kind: ChunkSourceKind::Documentation,
                    ordinal: dialog_index,
                    line_start: 1,
                    line_end: 1,
                    char_count: content.chars().count(),
                    text: content,
                    space_ids: BTreeSet::from([space_id.clone()]),
                    space_paths: BTreeMap::from([(space_id.clone(), graph.path_labels(&space_id))]),
                    hash: format!("hash-locomo-{sample_index}-{session_number}-{dialog_index}"),
                });
            }
            session_number += 1;
        }
    }
    let mut postings = BTreeMap::<String, Vec<TokenPosting>>::new();
    for chunk in &chunks {
        let mut frequencies = BTreeMap::<String, u16>::new();
        for token in benchmark_tokenize(&format!("{} {}", chunk.source_path, chunk.text)) {
            *frequencies.entry(token).or_insert(0) += 1;
        }
        for (token, frequency) in frequencies {
            postings.entry(token).or_default().push(TokenPosting {
                chunk_id: chunk.id.clone(),
                frequency,
            });
        }
    }
    let vector = vector_index_for_chunks(&chunks, embedding)?;
    Ok(IndexState {
        version: 1,
        full_text: FullTextIndex {
            version: 1,
            postings,
        },
        vector,
        records,
        chunks,
    })
}

fn benchmark_locomo(args: &[String], cwd: &Path) -> Result<String, String> {
    let total_start = Instant::now();
    let Some(data_path) = benchmark_arg_value(args, "--data").or_else(|| args.first().cloned())
    else {
        return Ok(json_object([
            ("benchmark".to_string(), quote("locomo")),
            ("dataset".to_string(), quote("locomo")),
            ("status".to_string(), quote("blocked")),
            ("pass".to_string(), false.to_string()),
            ("reason".to_string(), quote("missing_data_path")),
            (
                "usage".to_string(),
                quote(
                    "colmem benchmark locomo --data <locomo10.json> [--limit n] [--granularity session|dialog]",
                ),
            ),
        ]));
    };
    let data_path = PathBuf::from(data_path);
    if !data_path.exists() {
        return Ok(json_object([
            ("benchmark".to_string(), quote("locomo")),
            ("dataset".to_string(), quote("locomo")),
            ("status".to_string(), quote("blocked")),
            ("pass".to_string(), false.to_string()),
            ("reason".to_string(), quote("data_file_not_found")),
            (
                "data_path".to_string(),
                quote(&data_path.display().to_string()),
            ),
        ]));
    }
    let raw = std::fs::read_to_string(&data_path).map_err(|err| err.to_string())?;
    let samples = serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}'))
        .map_err(|err| err.to_string())?;
    let samples = samples
        .as_array()
        .ok_or_else(|| "LoCoMo data root must be a JSON array".to_string())?;
    let limit = benchmark_arg_value(args, "--limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(samples.len());
    let granularity =
        benchmark_arg_value(args, "--granularity").unwrap_or_else(|| "session".to_string());
    if !matches!(granularity.as_str(), "session" | "dialog") {
        return Err("usage: colmem benchmark locomo --data <locomo10.json> [--limit n] [--granularity session|dialog]".to_string());
    }
    let fusion = benchmark_arg_value(args, "--fusion").unwrap_or_else(|| "single".to_string());
    if !matches!(fusion.as_str(), "single" | "two-stage") {
        return Err("usage: colmem benchmark locomo --data <locomo10.json> [--limit n] [--granularity session|dialog] [--fusion single|two-stage]".to_string());
    }
    let embedding =
        benchmark_arg_value(args, "--embedding").unwrap_or_else(|| "signature".to_string());
    if !matches!(embedding.as_str(), "signature" | "semantic" | "remote") {
        return Err("usage: colmem benchmark locomo --data <locomo10.json> [--limit n] [--granularity session|dialog] [--embedding signature|semantic|remote]".to_string());
    }
    if embedding == "semantic" && !cfg!(feature = "semantic-embeddings") {
        return Ok(json_object([
            ("benchmark".to_string(), quote("locomo")),
            ("dataset".to_string(), quote("locomo")),
            ("status".to_string(), quote("blocked")),
            ("pass".to_string(), false.to_string()),
            (
                "reason".to_string(),
                quote("semantic_embeddings_feature_disabled"),
            ),
            ("required_feature".to_string(), quote("semantic-embeddings")),
        ]));
    }
    if embedding == "remote" && !cfg!(feature = "remote-embeddings") {
        return Ok(json_object([
            ("benchmark".to_string(), quote("locomo")),
            ("dataset".to_string(), quote("locomo")),
            ("status".to_string(), quote("blocked")),
            ("pass".to_string(), false.to_string()),
            (
                "reason".to_string(),
                quote("remote_embeddings_feature_disabled"),
            ),
            ("required_feature".to_string(), quote("remote-embeddings")),
        ]));
    }
    if embedding == "remote"
        && env::var("COLMEM_EMBEDDING_API_KEY").is_err()
        && env::var("MODELSCOPE_API_KEY").is_err()
    {
        return Ok(json_object([
            ("benchmark".to_string(), quote("locomo")),
            ("dataset".to_string(), quote("locomo")),
            ("status".to_string(), quote("blocked")),
            ("pass".to_string(), false.to_string()),
            (
                "reason".to_string(),
                quote("remote_embedding_api_key_missing"),
            ),
            (
                "required_env".to_string(),
                quote("COLMEM_EMBEDDING_API_KEY or MODELSCOPE_API_KEY"),
            ),
        ]));
    }
    let dialog_window = benchmark_arg_value(args, "--dialog-window")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .min(5);
    let query_feature_rerank = args
        .iter()
        .position(|arg| arg == "--query-feature-rerank")
        .map(|index| {
            args.get(index + 1)
                .filter(|value| !value.starts_with("--"))
                .cloned()
                .unwrap_or_else(|| "balanced".to_string())
        })
        .unwrap_or_else(|| "off".to_string());
    if !matches!(
        query_feature_rerank.as_str(),
        "off" | "balanced" | "conservative"
    ) {
        return Err("usage: --query-feature-rerank [balanced|conservative]".to_string());
    }

    let store = WorkspaceStateStore::new(cwd);
    let state = store.load_or_bootstrap()?;
    let mut project = state
        .primary_project()
        .cloned()
        .ok_or_else(|| "no project in workspace".to_string())?;
    project.id = "locomo".to_string();
    project.name = "LoCoMo".to_string();
    let agent = state
        .agents
        .iter()
        .find(|agent| agent.id == "builder")
        .cloned()
        .ok_or_else(|| "missing builder agent".to_string())?;
    let host = find_host(&HostId::Codex).ok_or_else(|| "missing codex host".to_string())?;

    let mut total_questions = 0usize;
    let mut answered_questions = 0usize;
    let mut evidence_hits_at_1 = 0usize;
    let mut evidence_hits_at_5 = 0usize;
    let mut evidence_hits_at_10 = 0usize;
    let mut evidence_hits_at_50 = 0usize;
    let mut top50_saturated_questions = 0usize;
    let mut candidate_pool_min = usize::MAX;
    let mut candidate_pool_max = 0usize;
    let mut candidate_pool_sum = 0usize;
    let mut candidate_pool_sizes = Vec::<usize>::new();
    let mut gold_absent_from_candidates = 0usize;
    let mut gold_present_pre_rerank_outside_top10 = 0usize;
    let mut gold_present_final_top10_outside_top5 = 0usize;
    let mut gold_moved_down_by_rerank = 0usize;
    let mut two_stage_session_misses = 0usize;
    let mut category_metrics = BTreeMap::<String, [usize; 5]>::new();
    let host_context = HostContext::new(host);

    for (sample_index, sample) in samples.iter().take(limit).enumerate() {
        let index = locomo_index_for_sample(
            sample_index,
            sample,
            &project.id,
            &state.spaces,
            if fusion == "two-stage" {
                "session"
            } else {
                &granularity
            },
            &embedding,
            dialog_window,
        )?;
        let mut harness = standard_harness();
        harness.registry = state.registry.clone();
        harness.graph = state.spaces.clone();
        harness.facts = state.facts.clone();
        harness.index = index;
        let Some(qa_pairs) = sample.get("qa").and_then(Value::as_array) else {
            continue;
        };
        for qa in qa_pairs {
            let Some(question) = qa.get("question").and_then(Value::as_str) else {
                continue;
            };
            let raw_evidence = qa
                .get("evidence")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            if raw_evidence.is_empty() {
                continue;
            }
            let evidence = locomo_evidence_ids(&raw_evidence, &granularity);
            let evidence_sessions = locomo_evidence_ids(&raw_evidence, "session");
            if evidence.is_empty() {
                continue;
            }
            total_questions += 1;
            let request = QueryRequest {
                text: question.to_string(),
                project_id: project.id.clone(),
                task_kind: TaskKind::Query,
                seed_space: agent.habitat.watch_spaces.iter().next().cloned(),
            };
            let retrieval_plan =
                harness
                    .retriever
                    .plan(&harness.graph, &project, &agent, &host_context, &request);
            let fact_hints = harness.facts.rerank_hints_for_query_scoped(
                question,
                FactQueryScope::All,
                &InMemoryFactStore::today_iso_utc(),
            );
            let diagnostics = harness.retriever.index_hits_with_diagnostics(
                &harness.index,
                &request,
                &retrieval_plan,
                &fact_hints,
                50,
            );
            let pre_rerank_hits = diagnostics.pre_rerank_hits;
            let candidate_count = diagnostics.candidate_count;
            let mut hits = diagnostics.hits;
            for hit in &mut hits {
                hit.space_path = harness.graph.path_labels(&hit.space_id);
            }
            if query_feature_rerank != "off" {
                locomo_rerank_hits_by_query_features(question, &mut hits, &query_feature_rerank);
            }
            let (hits, candidate_pool_size, pre_rerank_hits, session_stage_miss) =
                if granularity == "dialog" && fusion == "two-stage" {
                    let session_candidates = hits
                        .iter()
                        .take(5)
                        .map(|hit| hit.chunk_id.clone())
                        .collect::<BTreeSet<_>>();
                    let session_stage_miss = !evidence_sessions.is_empty()
                        && session_candidates
                            .intersection(&evidence_sessions)
                            .next()
                            .is_none();
                    let dialog_index = locomo_restricted_dialog_index_for_sample(
                        sample_index,
                        sample,
                        &project.id,
                        &state.spaces,
                        &session_candidates,
                        &embedding,
                        dialog_window,
                    )?;
                    let mut dialog_harness = standard_harness();
                    dialog_harness.registry = state.registry.clone();
                    dialog_harness.graph = state.spaces.clone();
                    dialog_harness.facts = state.facts.clone();
                    dialog_harness.index = dialog_index;
                    let request = QueryRequest {
                        text: question.to_string(),
                        project_id: project.id.clone(),
                        task_kind: TaskKind::Query,
                        seed_space: agent.habitat.watch_spaces.iter().next().cloned(),
                    };
                    let retrieval_plan = dialog_harness.retriever.plan(
                        &dialog_harness.graph,
                        &project,
                        &agent,
                        &host_context,
                        &request,
                    );
                    let fact_hints = dialog_harness.facts.rerank_hints_for_query_scoped(
                        question,
                        FactQueryScope::All,
                        &InMemoryFactStore::today_iso_utc(),
                    );
                    let diagnostics = dialog_harness.retriever.index_hits_with_diagnostics(
                        &dialog_harness.index,
                        &request,
                        &retrieval_plan,
                        &fact_hints,
                        50,
                    );
                    let pre_rerank_hits = diagnostics.pre_rerank_hits;
                    let candidate_count = diagnostics.candidate_count;
                    let mut hits = diagnostics.hits;
                    for hit in &mut hits {
                        hit.space_path = dialog_harness.graph.path_labels(&hit.space_id);
                    }
                    if query_feature_rerank != "off" {
                        locomo_rerank_hits_by_query_features(
                            question,
                            &mut hits,
                            &query_feature_rerank,
                        );
                    }
                    (hits, candidate_count, pre_rerank_hits, session_stage_miss)
                } else {
                    (hits, candidate_count, pre_rerank_hits, false)
                };
            answered_questions += 1;
            candidate_pool_min = candidate_pool_min.min(candidate_pool_size);
            candidate_pool_max = candidate_pool_max.max(candidate_pool_size);
            candidate_pool_sum += candidate_pool_size;
            candidate_pool_sizes.push(candidate_pool_size);
            if candidate_pool_size <= 50 {
                top50_saturated_questions += 1;
            }
            if session_stage_miss {
                two_stage_session_misses += 1;
            }
            let pre_rerank_rank = first_evidence_rank(&pre_rerank_hits, &evidence);
            let final_rank = first_evidence_rank(&hits, &evidence);
            match pre_rerank_rank {
                Some(rank) if rank > 10 => gold_present_pre_rerank_outside_top10 += 1,
                None => gold_absent_from_candidates += 1,
                _ => {}
            }
            if matches!(final_rank, Some(6..=10)) {
                gold_present_final_top10_outside_top5 += 1;
            }
            if pre_rerank_rank
                .is_some_and(|rank| final_rank.is_none_or(|final_rank| final_rank > rank))
            {
                gold_moved_down_by_rerank += 1;
            }
            let category = qa
                .get("category")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_else(|| "unknown".to_string());
            let hit_at_1 = hits
                .iter()
                .take(1)
                .any(|hit| evidence.contains(&hit.chunk_id));
            let hit_at_5 = hits
                .iter()
                .take(5)
                .any(|hit| evidence.contains(&hit.chunk_id));
            let hit_at_10 = hits
                .iter()
                .take(10)
                .any(|hit| evidence.contains(&hit.chunk_id));
            let hit_at_50 = hits
                .iter()
                .take(50)
                .any(|hit| evidence.contains(&hit.chunk_id));
            if hit_at_1 {
                evidence_hits_at_1 += 1;
            }
            if hit_at_5 {
                evidence_hits_at_5 += 1;
            }
            if hit_at_10 {
                evidence_hits_at_10 += 1;
            }
            if hit_at_50 {
                evidence_hits_at_50 += 1;
            }
            let metrics = category_metrics.entry(category).or_insert([0; 5]);
            metrics[0] += 1;
            metrics[1] += usize::from(hit_at_1);
            metrics[2] += usize::from(hit_at_5);
            metrics[3] += usize::from(hit_at_10);
            metrics[4] += usize::from(hit_at_50);
        }
    }
    let recall = |hits: usize| {
        if total_questions == 0 {
            0.0
        } else {
            hits as f64 / total_questions as f64
        }
    };
    candidate_pool_sizes.sort_unstable();
    let candidate_pool_median = candidate_pool_sizes
        .get(candidate_pool_sizes.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or_default();
    let candidate_pool_avg = if answered_questions == 0 {
        0.0
    } else {
        candidate_pool_sum as f64 / answered_questions as f64
    };
    let per_category = format!(
        "{{{}}}",
        category_metrics
            .iter()
            .map(|(category, metrics)| {
                let category_recall = |hits: usize| {
                    if metrics[0] == 0 {
                        0.0
                    } else {
                        hits as f64 / metrics[0] as f64
                    }
                };
                format!(
                    "{}: {}",
                    quote(category),
                    json_object([
                        ("questions".to_string(), metrics[0].to_string()),
                        ("evidence_hits_at_1".to_string(), metrics[1].to_string()),
                        ("evidence_hits_at_5".to_string(), metrics[2].to_string()),
                        ("evidence_hits_at_10".to_string(), metrics[3].to_string()),
                        ("evidence_hits_at_50".to_string(), metrics[4].to_string()),
                        (
                            "recall_at_1".to_string(),
                            format!("{:.3}", category_recall(metrics[1])),
                        ),
                        (
                            "recall_at_5".to_string(),
                            format!("{:.3}", category_recall(metrics[2])),
                        ),
                        (
                            "recall_at_10".to_string(),
                            format!("{:.3}", category_recall(metrics[3])),
                        ),
                        (
                            "recall_at_50".to_string(),
                            format!("{:.3}", category_recall(metrics[4])),
                        ),
                    ])
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(json_object([
        ("benchmark".to_string(), quote("locomo")),
        ("dataset".to_string(), quote("locomo")),
        (
            "mode".to_string(),
            quote(&format!("{granularity}_retrieval")),
        ),
        ("granularity".to_string(), quote(&granularity)),
        ("fusion".to_string(), quote(&fusion)),
        ("embedding".to_string(), quote(&embedding)),
        ("dialog_window".to_string(), dialog_window.to_string()),
        (
            "query_feature_rerank".to_string(),
            quote(&query_feature_rerank),
        ),
        ("status".to_string(), quote("completed")),
        ("pass".to_string(), (total_questions > 0).to_string()),
        (
            "data_path".to_string(),
            quote(&data_path.display().to_string()),
        ),
        ("limit".to_string(), limit.to_string()),
        (
            "elapsed_ms".to_string(),
            total_start.elapsed().as_millis().to_string(),
        ),
        (
            "metrics".to_string(),
            json_object([
                ("questions".to_string(), total_questions.to_string()),
                ("answered".to_string(), answered_questions.to_string()),
                (
                    "candidate_pool_min".to_string(),
                    if candidate_pool_min == usize::MAX {
                        "0".to_string()
                    } else {
                        candidate_pool_min.to_string()
                    },
                ),
                (
                    "candidate_pool_max".to_string(),
                    candidate_pool_max.to_string(),
                ),
                (
                    "candidate_pool_median".to_string(),
                    candidate_pool_median.to_string(),
                ),
                (
                    "candidate_pool_avg".to_string(),
                    format!("{candidate_pool_avg:.1}"),
                ),
                (
                    "top50_saturated_questions".to_string(),
                    top50_saturated_questions.to_string(),
                ),
                (
                    "recall_at_50_saturated".to_string(),
                    (top50_saturated_questions > 0).to_string(),
                ),
                (
                    "gold_absent_from_candidates".to_string(),
                    gold_absent_from_candidates.to_string(),
                ),
                (
                    "gold_present_pre_rerank_outside_top10".to_string(),
                    gold_present_pre_rerank_outside_top10.to_string(),
                ),
                (
                    "gold_present_final_top10_outside_top5".to_string(),
                    gold_present_final_top10_outside_top5.to_string(),
                ),
                (
                    "gold_moved_down_by_rerank".to_string(),
                    gold_moved_down_by_rerank.to_string(),
                ),
                (
                    "two_stage_session_misses".to_string(),
                    two_stage_session_misses.to_string(),
                ),
                (
                    "evidence_hits_at_1".to_string(),
                    evidence_hits_at_1.to_string(),
                ),
                (
                    "evidence_hits_at_5".to_string(),
                    evidence_hits_at_5.to_string(),
                ),
                (
                    "evidence_hits_at_10".to_string(),
                    evidence_hits_at_10.to_string(),
                ),
                (
                    "evidence_hits_at_50".to_string(),
                    evidence_hits_at_50.to_string(),
                ),
                (
                    "recall_at_1".to_string(),
                    format!("{:.3}", recall(evidence_hits_at_1)),
                ),
                (
                    "recall_at_5".to_string(),
                    format!("{:.3}", recall(evidence_hits_at_5)),
                ),
                (
                    "recall_at_10".to_string(),
                    format!("{:.3}", recall(evidence_hits_at_10)),
                ),
                (
                    "recall_at_50".to_string(),
                    format!("{:.3}", recall(evidence_hits_at_50)),
                ),
                ("per_category".to_string(), per_category),
            ]),
        ),
    ]))
}

fn cmd_ingest(args: &[String], cwd: &Path) -> Result<String, String> {
    let store = WorkspaceStateStore::new(cwd);
    let mut state = store.load_or_bootstrap()?;
    let project_id = args.first().map(String::as_str).unwrap_or("colmem");
    let project = state
        .project_by_id(project_id)
        .cloned()
        .or_else(|| state.primary_project().cloned())
        .ok_or_else(|| "no project in workspace".to_string())?;

    let (index, summary) = build_project_index(&project)?;
    state.index = index;
    store.save(&state)?;

    Ok(format!(
        "Indexed project '{}'\nrecords={}\nchunks={}\nskipped_files={}\nstate={}",
        project.name,
        summary.records,
        summary.chunks,
        summary.skipped_files,
        store.paths.state_file.display()
    ))
}

fn cmd_query(args: &[String], cwd: &Path) -> Result<String, String> {
    if args.is_empty() {
        return Err("usage: colmem query <text> [host] [agent]".to_string());
    }

    let query = args[0].clone();
    let host_id = args
        .get(1)
        .and_then(|raw| HostId::from_str(raw).ok())
        .unwrap_or(HostId::Codex);
    let agent_id = args.get(2).map(String::as_str).unwrap_or("builder");
    let host = find_host(&host_id).ok_or_else(|| "unknown host".to_string())?;
    let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;
    let agent = state
        .agents
        .iter()
        .find(|candidate| candidate.id == agent_id)
        .cloned()
        .ok_or_else(|| format!("unknown agent: {agent_id}"))?;
    let project = state
        .primary_project()
        .cloned()
        .ok_or_else(|| "no project in workspace".to_string())?;
    let mut harness = standard_harness();
    harness.registry = state.registry.clone();
    harness.graph = state.spaces.clone();
    harness.facts = state.facts.clone();
    harness.index = state.index.clone();
    let snapshot = harness.prepare_run(
        &agent,
        &project,
        &HostContext::new(host),
        &TaskIntent {
            kind: TaskKind::Query,
            summary: query,
            requested_capabilities: Default::default(),
        },
    );
    Ok(snapshot.to_json())
}

fn cmd_facts(args: &[String], cwd: &Path) -> Result<String, String> {
    let store = WorkspaceStateStore::new(cwd);

    match args.first().map(String::as_str) {
        None | Some("list") => {
            let state = store.load_or_bootstrap()?;
            let (scope, reference_date) = parse_fact_scope_and_date(args.get(1), args.get(2))?;
            Ok(state
                .facts
                .facts_scoped(scope, &reference_date)
                .into_iter()
                .map(|fact| fact.to_json_with_status(&reference_date))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("summary") => {
            let reference_date = args
                .get(1)
                .cloned()
                .unwrap_or_else(InMemoryFactStore::today_iso_utc);
            let state = store.load_or_bootstrap()?;
            Ok(state.facts.summary_json(&reference_date))
        }
        Some("query") => {
            let query = args
                .get(1)
                .ok_or_else(|| "usage: colmem facts query <text>".to_string())?;
            let (scope, reference_date) = parse_fact_scope_and_date(args.get(2), args.get(3))?;
            let state = store.load_or_bootstrap()?;
            Ok(state
                .facts
                .facts_for_query_scoped(query, scope, &reference_date)
                .into_iter()
                .map(|fact| fact.to_json_with_status(&reference_date))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("add") => {
            let subject = args
                .get(1)
                .cloned()
                .ok_or_else(|| "usage: colmem facts add <subject> <predicate> <object> [confidence] [valid_from] [valid_to] [evidence_refs]".to_string())?;
            let predicate = args
                .get(2)
                .cloned()
                .ok_or_else(|| "usage: colmem facts add <subject> <predicate> <object> [confidence] [valid_from] [valid_to] [evidence_refs]".to_string())?;
            let object = args
                .get(3)
                .cloned()
                .ok_or_else(|| "usage: colmem facts add <subject> <predicate> <object> [confidence] [valid_from] [valid_to] [evidence_refs]".to_string())?;
            let confidence = args
                .get(4)
                .and_then(|raw| raw.parse::<u8>().ok())
                .unwrap_or(80);
            let valid_from = args.get(5).cloned().filter(|value| value != "-");
            let valid_to = args.get(6).cloned().filter(|value| value != "-");
            let explicit_evidence_refs = args.get(7).map(|raw| {
                raw.split(',')
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| value.trim().to_string())
                    .collect::<Vec<_>>()
            });
            let evidence_ids = explicit_evidence_refs.unwrap_or_else(|| {
                vec![format!(
                    "manual-{}-{}-{}",
                    subject.to_ascii_lowercase(),
                    predicate.to_ascii_lowercase(),
                    object.to_ascii_lowercase().replace(' ', "-")
                )]
            });

            let mut state = store.load_or_bootstrap()?;
            let result = state.facts.apply_add_with_policy(
                &FactWritePolicy,
                Fact {
                    subject,
                    predicate,
                    object,
                    valid_from,
                    valid_to,
                    confidence,
                    evidence_ids,
                },
            );
            store.save(&state)?;
            Ok(result.to_json())
        }
        Some("update") => {
            let subject = args
                .get(1)
                .cloned()
                .ok_or_else(|| "usage: colmem facts update <subject> <predicate> <object> [confidence] [valid_from] [evidence_refs]".to_string())?;
            let predicate = args
                .get(2)
                .cloned()
                .ok_or_else(|| "usage: colmem facts update <subject> <predicate> <object> [confidence] [valid_from] [evidence_refs]".to_string())?;
            let object = args
                .get(3)
                .cloned()
                .ok_or_else(|| "usage: colmem facts update <subject> <predicate> <object> [confidence] [valid_from] [evidence_refs]".to_string())?;
            let confidence = args
                .get(4)
                .and_then(|raw| raw.parse::<u8>().ok())
                .unwrap_or(80);
            let valid_from = args
                .get(5)
                .cloned()
                .filter(|value| value != "-")
                .or_else(|| Some(colmem_core::facts::InMemoryFactStore::today_iso_utc()));
            let effective_date = valid_from
                .clone()
                .unwrap_or_else(colmem_core::facts::InMemoryFactStore::today_iso_utc);
            let evidence_ids = args
                .get(6)
                .map(|raw| {
                    raw.split(',')
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| value.trim().to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| {
                    vec![format!(
                        "manual-{}-{}-{}",
                        subject.to_ascii_lowercase(),
                        predicate.to_ascii_lowercase(),
                        object.to_ascii_lowercase().replace(' ', "-")
                    )]
                });

            let mut state = store.load_or_bootstrap()?;
            let result = state.facts.apply_update_with_policy(
                &FactWritePolicy,
                Fact {
                    subject,
                    predicate,
                    object,
                    valid_from,
                    valid_to: None,
                    confidence,
                    evidence_ids,
                },
                &effective_date,
            );
            store.save(&state)?;
            Ok(result.to_json())
        }
        Some("invalidate") => {
            let subject = args.get(1).cloned().ok_or_else(|| {
                "usage: colmem facts invalidate <subject> <predicate> [object] [valid_to]"
                    .to_string()
            })?;
            let predicate = args.get(2).cloned().ok_or_else(|| {
                "usage: colmem facts invalidate <subject> <predicate> [object] [valid_to]"
                    .to_string()
            })?;
            let object = args.get(3).cloned().filter(|value| value != "-");
            let valid_to = args
                .get(4)
                .cloned()
                .filter(|value| value != "-")
                .unwrap_or_else(colmem_core::facts::InMemoryFactStore::today_iso_utc);

            let mut state = store.load_or_bootstrap()?;
            let result = state.facts.apply_invalidate_with_policy(
                &FactWritePolicy,
                &subject,
                &predicate,
                object.as_deref(),
                &valid_to,
            );
            store.save(&state)?;
            Ok(result.to_json())
        }
        Some("audit") => {
            let state = store.load_or_bootstrap()?;
            let events = if let Some(query) = args.get(1) {
                state.facts.audit_events_for_query(query)
            } else {
                state.facts.audit_log().to_vec()
            };
            Ok(events
                .into_iter()
                .map(|event| event.to_json())
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some(other) => Err(format!("unknown facts subcommand: {other}")),
    }
}

fn parse_fact_scope_and_date(
    raw_scope: Option<&String>,
    raw_date: Option<&String>,
) -> Result<(FactQueryScope, String), String> {
    let scope = match raw_scope.map(String::as_str) {
        None => FactQueryScope::All,
        Some("all") => FactQueryScope::All,
        Some("active") => FactQueryScope::Active,
        Some("history") => FactQueryScope::History,
        Some("scheduled") => FactQueryScope::Scheduled,
        Some(other) => {
            return Err(format!(
                "unknown fact query scope: {other} (expected active|history|scheduled|all)"
            ));
        }
    };
    let reference_date = raw_date
        .cloned()
        .unwrap_or_else(InMemoryFactStore::today_iso_utc);
    Ok((scope, reference_date))
}

fn cmd_index(args: &[String], cwd: &Path) -> Result<String, String> {
    let state = WorkspaceStateStore::new(cwd).load_or_bootstrap()?;

    match args.first().map(String::as_str) {
        None | Some("inspect") => {
            if let Some(chunk_id) = args.get(1) {
                let chunk = state
                    .index
                    .chunks
                    .iter()
                    .find(|chunk| &chunk.id == chunk_id)
                    .ok_or_else(|| format!("unknown chunk: {chunk_id}"))?;
                Ok(chunk.to_json())
            } else {
                Ok(state.index.inspect_json())
            }
        }
        Some(other) => Err(format!("unknown index subcommand: {other}")),
    }
}

fn cmd_mcp(args: &[String], cwd: &Path) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("serve") => {
            colmem_core::mcp::serve_stdio(&cwd.display().to_string())
                .map_err(|err| err.to_string())?;
            Ok(String::new())
        }
        Some(other) => Err(format!("unknown mcp subcommand: {other}")),
        None => Err("usage: colmem mcp serve".to_string()),
    }
}

fn help_text() -> String {
    [
        "colmem <command>",
        "  init [path]",
        "  host list",
        "  host inspect <host>",
        "  host install <host> [workspace_root]",
        "  host install-all [workspace_root]",
        "  host diagnostics <host> [workspace_root]",
        "  host acceptance <host> [workspace_root]",
        "  host verify <host> [workspace_root]",
        "  host verify-all [workspace_root]",
        "  host smoke <host> [workspace_root]",
        "  host smoke-all [workspace_root]",
        "  capability list",
        "  project attach <name> [path]",
        "  project inspect",
        "  project ingest-policy [project_id]",
        "  project ingest-policy update <field> <add|remove> <value> [project_id]",
        "  project rerank-source-weights [project_id]",
        "  project rerank-source-weights update <field> <value> [project_id]",
        "  ingest [project_id]",
        "  index inspect [chunk_id]",
        "  facts list [active|history|scheduled|all] [reference_date]",
        "  facts summary [reference_date]",
        "  facts query <text> [active|history|scheduled|all] [reference_date]",
        "  facts add <subject> <predicate> <object> [confidence] [valid_from] [valid_to] [evidence_refs]",
        "  facts update <subject> <predicate> <object> [confidence] [valid_from] [evidence_refs]",
        "  facts invalidate <subject> <predicate> [object] [valid_to]",
        "  facts audit [text]",
        "  agent inspect [id]",
        "  agent evolve [id]",
        "  memory map [space_id]",
        "  benchmark smoke",
        "  benchmark synthetic [--size smoke|small]",
        "  benchmark locomo --data <locomo10.json> [--limit n] [--granularity session|dialog] [--fusion single|two-stage] [--embedding signature|semantic|remote] [--dialog-window n] [--query-feature-rerank [balanced|conservative]]",
        "  query <text> [host] [agent]",
        "  mcp serve",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::run_in_dir;

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("{prefix}-{stamp}"));
        root
    }

    fn create_fixture_workspace() -> PathBuf {
        let root = temp_dir("colmem-cli-test");
        fs::create_dir_all(root.join("src")).expect("create src dir");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn remember_fact() -> &'static str { \"colmem cli integration token\" }\n",
        )
        .expect("write fixture source");
        root
    }

    #[test]
    fn init_bootstraps_workspace_from_test_directory() {
        let root = temp_dir("colmem-cli-init");
        let output = run_in_dir(vec!["init".to_string()], &root).expect("init workspace");
        assert!(output.contains("Initialized colmem workspace"));
        assert!(root.join(".colmem/workspace-state.json").exists());
    }

    #[test]
    fn host_list_is_testable_without_live_binary() {
        let output = run_in_dir(vec!["host".to_string(), "list".to_string()], Path::new("."))
            .expect("host list");
        assert!(output.contains("Codex [codex]"));
        assert!(output.contains("Cursor [cursor]"));
    }

    #[test]
    fn host_install_outputs_plan_without_writing_host_files() {
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "install".to_string(),
                "openclaw".to_string(),
                "D:/repo/colmem".to_string(),
            ],
            Path::new("."),
        )
        .expect("host install plan");

        assert!(output.contains("\"host\""));
        assert!(output.contains("\"command\": \"colmem mcp serve\""));
        assert!(output.contains("\"config_format\": \"json:mcpServers\""));
        assert!(output.contains("\"acceptance_checks\""));
        assert!(output.contains("\"acceptance_plan\""));
        assert!(output.contains("\"action\": \"tools/list\""));
        assert!(output.contains("\"request_json\""));
        assert!(output.contains("\\\"method\\\":\\\"tools/list\\\""));
        assert!(output.contains("D:/repo/colmem"));
    }

    #[test]
    fn host_install_defaults_to_run_directory() {
        let root = temp_dir("colmem-cli-host-install");
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "install".to_string(),
                "codex".to_string(),
            ],
            &root,
        )
        .expect("host install plan");

        assert!(output.contains("\"config_format\": \"toml:mcp_servers\""));
        assert!(output.contains(&root.display().to_string().replace('\\', "\\\\")));
    }

    #[test]
    fn host_install_all_outputs_dry_run_plans_for_all_hosts() {
        let root = temp_dir("colmem-cli-host-install-all");
        let output = run_in_dir(vec!["host".to_string(), "install-all".to_string()], &root)
            .expect("host install all");

        assert!(output.contains("\"mode\": \"dry_run_all_hosts\""));
        assert!(output.contains("\"writes_files\": false"));
        assert!(output.contains("\"host\": {\"id\": \"claude_code\""));
        assert!(output.contains("\"host\": {\"id\": \"codex\""));
        assert!(output.contains("\"host\": {\"id\": \"cursor\""));
        assert!(output.contains("\"host\": {\"id\": \"trae_ide\""));
        assert!(output.contains("\"host\": {\"id\": \"openclaw\""));
    }

    #[test]
    fn host_diagnostics_outputs_transport_checks() {
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "diagnostics".to_string(),
                "trae".to_string(),
                "D:/repo/colmem".to_string(),
            ],
            Path::new("."),
        )
        .expect("host diagnostics");

        assert!(output.contains("transport=cli"));
        assert!(output.contains("config_format=json:cli_plugin"));
        assert!(output.contains("plugins_not_supported_by_host_descriptor"));
    }

    #[test]
    fn host_acceptance_outputs_smoke_checks() {
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "acceptance".to_string(),
                "cursor".to_string(),
                "D:/repo/colmem".to_string(),
            ],
            Path::new("."),
        )
        .expect("host acceptance checks");

        assert!(output.contains("colmem_query_plan"));
        assert!(output.contains("colmem_agent_inspect"));
        assert!(output.contains("colmem_capability_list"));
        assert!(output.contains("colmem_memory_map"));
        assert!(output.contains("tools/call"));
        assert!(output.contains("cursor"));
    }

    #[test]
    fn host_verify_outputs_static_compatibility_report() {
        let root = temp_dir("colmem-cli-host-verify");
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "verify".to_string(),
                "codex".to_string(),
            ],
            &root,
        )
        .expect("host verify");

        assert!(output.contains("\"host\": \"codex\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"id\": \"workspace_root_exists\""));
        assert!(output.contains("\"id\": \"acceptance_plan_available\""));
        assert!(output.contains("steps=7"));
    }

    #[test]
    fn host_verify_all_outputs_static_reports_for_all_hosts() {
        let root = temp_dir("colmem-cli-host-verify-all");
        let output = run_in_dir(vec!["host".to_string(), "verify-all".to_string()], &root)
            .expect("host verify all");

        assert!(output.contains("\"mode\": \"static_all_hosts\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"host\": \"claude_code\""));
        assert!(output.contains("\"host\": \"codex\""));
        assert!(output.contains("\"host\": \"cursor\""));
        assert!(output.contains("\"host\": \"trae_ide\""));
        assert!(output.contains("\"host\": \"openclaw\""));
    }

    #[test]
    fn host_smoke_runs_in_process_mcp_acceptance_requests() {
        let root = temp_dir("colmem-cli-host-smoke");
        let output = run_in_dir(
            vec![
                "host".to_string(),
                "smoke".to_string(),
                "openclaw".to_string(),
            ],
            &root,
        )
        .expect("host smoke");

        assert!(output.contains("\"mode\": \"in_process_mcp\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"id\": \"mcp_tools_list\""));
        assert!(output.contains("\"id\": \"query_plan\""));
        assert!(output.contains("\"id\": \"agent_inspect\""));
        assert!(output.contains("\"id\": \"capability_list\""));
        assert!(output.contains("\"id\": \"memory_map\""));
    }

    #[test]
    fn host_smoke_all_runs_all_builtin_hosts_in_process() {
        let root = temp_dir("colmem-cli-host-smoke-all");
        let output = run_in_dir(vec!["host".to_string(), "smoke-all".to_string()], &root)
            .expect("host smoke all");

        assert!(output.contains("\"mode\": \"in_process_mcp_all_hosts\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"host\": \"claude_code\""));
        assert!(output.contains("\"host\": \"codex\""));
        assert!(output.contains("\"host\": \"cursor\""));
        assert!(output.contains("\"host\": \"trae_ide\""));
        assert!(output.contains("\"host\": \"openclaw\""));
    }

    #[test]
    fn capability_list_reads_bootstrapped_state_from_test_workspace() {
        let root = temp_dir("colmem-cli-capability");
        let output = run_in_dir(vec!["capability".to_string(), "list".to_string()], &root)
            .expect("capability list");
        assert!(output.contains("\"repo_search\""));
        assert!(root.join(".colmem/workspace-state.json").exists());
    }

    #[test]
    fn memory_map_outputs_structured_space_paths() {
        let root = temp_dir("colmem-cli-memory-map");
        let output =
            run_in_dir(vec!["memory".to_string(), "map".to_string()], &root).expect("memory map");

        assert!(output.contains("\"nodes\""));
        assert!(output.contains("\"links\""));
        assert!(output.contains("\"id\": \"retrieval\""));
        assert!(output.contains("\"memory_path\": \"Workspace Root > Architecture > Retrieval\""));
    }

    #[test]
    fn memory_map_can_filter_by_space_id() {
        let root = temp_dir("colmem-cli-memory-map-filter");
        let output = run_in_dir(
            vec![
                "memory".to_string(),
                "map".to_string(),
                "retrieval".to_string(),
            ],
            &root,
        )
        .expect("filtered memory map");

        assert!(output.contains("\"id\": \"retrieval\""));
        assert!(output.contains("\"memory_path\": \"Workspace Root > Architecture > Retrieval\""));
        assert!(!output.contains("\"id\": \"host_adapters\""));
    }

    #[test]
    fn benchmark_smoke_runs_core_runtime_checks() {
        let root = temp_dir("colmem-cli-benchmark-smoke");
        let output = run_in_dir(vec!["benchmark".to_string(), "smoke".to_string()], &root)
            .expect("benchmark smoke");

        assert!(output.contains("\"benchmark\": \"smoke\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"id\": \"memory_map\""));
        assert!(output.contains("\"id\": \"query_plan\""));
        assert!(output.contains("\"id\": \"host_smoke_all\""));
    }

    #[test]
    fn benchmark_synthetic_runs_deterministic_scoring() {
        let root = temp_dir("colmem-cli-benchmark-synthetic");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "synthetic".to_string(),
                "--size".to_string(),
                "smoke".to_string(),
            ],
            &root,
        )
        .expect("benchmark synthetic");

        assert!(output.contains("\"benchmark\": \"synthetic\""));
        assert!(output.contains("\"dataset\": \"synthetic\""));
        assert!(output.contains("\"pass\": true"));
        assert!(output.contains("\"top_hit_contains_needle\": true"));
        assert!(output.contains("\"memory_path_match_total\""));
    }

    #[test]
    fn benchmark_locomo_runs_tiny_fixture() {
        let root = temp_dir("colmem-cli-benchmark-locomo");
        fs::create_dir_all(&root).expect("create temp dir");
        let data = root.join("locomo_fixture.json");
        std::fs::write(
            &data,
            r#"[{
                "conversation": {
                    "session_1_date_time": "2026/04/13",
                    "session_1": [
                        {"dia_id": "D1:1", "speaker": "Alice", "text": "The project codename is blue lantern."},
                        {"dia_id": "D1:2", "speaker": "Bob", "text": "We should deploy the dashboard tomorrow."}
                    ]
                },
                "qa": [
                    {
                        "question": "What is the project codename?",
                        "answer": "blue lantern",
                        "category": "single_hop",
                        "evidence": ["D1:1"]
                    }
                ]
            }]"#,
        )
        .expect("write locomo fixture");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "locomo".to_string(),
                "--data".to_string(),
                data.display().to_string(),
                "--limit".to_string(),
                "1".to_string(),
            ],
            &root,
        )
        .expect("benchmark locomo");

        assert!(output.contains("\"benchmark\": \"locomo\""));
        assert!(output.contains("\"status\": \"completed\""));
        assert!(output.contains("\"questions\": 1"));
        assert!(output.contains("\"candidate_pool_min\": 1"));
        assert!(output.contains("\"candidate_pool_max\": 1"));
        assert!(output.contains("\"candidate_pool_median\": 1"));
        assert!(output.contains("\"candidate_pool_avg\": 1.0"));
        assert!(output.contains("\"query_feature_rerank\": \"off\""));
        assert!(output.contains("\"per_category\""));
        assert!(output.contains("\"single_hop\""));
        assert!(output.contains("\"recall_at_5\""));
    }

    #[test]
    fn benchmark_locomo_conservative_query_feature_rerank_runs_tiny_fixture() {
        let root = temp_dir("colmem-cli-benchmark-locomo-conservative-rerank");
        fs::create_dir_all(&root).expect("create temp dir");
        let data = root.join("locomo_fixture.json");
        std::fs::write(
            &data,
            r#"[{
                "conversation": {
                    "session_1_date_time": "2026/04/13",
                    "session_1": [
                        {"dia_id": "D1:1", "speaker": "Alice", "text": "The project codename is blue lantern."},
                        {"dia_id": "D1:2", "speaker": "Bob", "text": "We should deploy the dashboard tomorrow."}
                    ]
                },
                "qa": [
                    {
                        "question": "What is the project codename?",
                        "answer": "blue lantern",
                        "category": "single_hop",
                        "evidence": ["D1:1"]
                    }
                ]
            }]"#,
        )
        .expect("write locomo fixture");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "locomo".to_string(),
                "--data".to_string(),
                data.display().to_string(),
                "--granularity".to_string(),
                "dialog".to_string(),
                "--query-feature-rerank".to_string(),
                "conservative".to_string(),
            ],
            &root,
        )
        .expect("benchmark locomo conservative query-feature rerank");

        assert!(output.contains("\"query_feature_rerank\": \"conservative\""));
        assert!(output.contains("\"per_category\""));
        assert!(output.contains("\"recall_at_5\": 1.000"));
    }

    #[test]
    fn benchmark_locomo_reports_blocked_when_data_missing() {
        let root = temp_dir("colmem-cli-benchmark-locomo-missing");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "locomo".to_string(),
                "--data".to_string(),
                root.join("missing.json").display().to_string(),
            ],
            &root,
        )
        .expect("benchmark locomo missing");

        assert!(output.contains("\"status\": \"blocked\""));
        assert!(output.contains("\"reason\": \"data_file_not_found\""));
    }

    #[test]
    fn benchmark_locomo_two_stage_runs_tiny_fixture() {
        let root = temp_dir("colmem-cli-benchmark-locomo-two-stage");
        fs::create_dir_all(&root).expect("create temp dir");
        let data = root.join("locomo_fixture.json");
        std::fs::write(
            &data,
            r#"[{
                "conversation": {
                    "session_1_date_time": "2026/04/13",
                    "session_1": [
                        {"dia_id": "D1:1", "speaker": "Alice", "text": "The project codename is blue lantern."}
                    ]
                },
                "qa": [
                    {
                        "question": "What is the project codename?",
                        "answer": "blue lantern",
                        "category": "single_hop",
                        "evidence": ["D1:1"]
                    }
                ]
            }]"#,
        )
        .expect("write locomo fixture");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "locomo".to_string(),
                "--data".to_string(),
                data.display().to_string(),
                "--granularity".to_string(),
                "dialog".to_string(),
                "--fusion".to_string(),
                "two-stage".to_string(),
            ],
            &root,
        )
        .expect("benchmark locomo two-stage");

        assert!(output.contains("\"granularity\": \"dialog\""));
        assert!(output.contains("\"fusion\": \"two-stage\""));
        assert!(output.contains("\"dialog_window\": 1"));
        assert!(output.contains("\"recall_at_5\": 1.000"));
    }

    #[test]
    fn benchmark_locomo_reports_blocked_when_remote_feature_disabled() {
        let root = temp_dir("colmem-cli-benchmark-locomo-remote-disabled");
        fs::create_dir_all(&root).expect("create temp dir");
        let data = root.join("locomo_fixture.json");
        std::fs::write(&data, "[]").expect("write locomo fixture");
        let output = run_in_dir(
            vec![
                "benchmark".to_string(),
                "locomo".to_string(),
                "--data".to_string(),
                data.display().to_string(),
                "--embedding".to_string(),
                "remote".to_string(),
            ],
            &root,
        )
        .expect("benchmark locomo remote disabled");

        if cfg!(feature = "remote-embeddings") {
            assert!(output.contains("\"reason\": \"remote_embedding_api_key_missing\""));
        } else {
            assert!(output.contains("\"status\": \"blocked\""));
            assert!(output.contains("\"reason\": \"remote_embeddings_feature_disabled\""));
        }
    }

    #[test]
    fn project_ingest_policy_is_inspectable_from_cli() {
        let root = temp_dir("colmem-cli-project-ingest-policy");
        let output = run_in_dir(
            vec!["project".to_string(), "ingest-policy".to_string()],
            &root,
        )
        .expect("project ingest policy");

        assert!(output.contains("\"skipped_dirs\""));
        assert!(output.contains("\"allowed_extensions\""));
        assert!(output.contains("\"IMPLEMENTATION_PLAN.md\""));
    }

    #[test]
    fn project_ingest_policy_update_persists_changes() {
        let root = temp_dir("colmem-cli-project-ingest-policy-update");
        let output = run_in_dir(
            vec![
                "project".to_string(),
                "ingest-policy".to_string(),
                "update".to_string(),
                "skipped_file_names".to_string(),
                "remove".to_string(),
                "IMPLEMENTATION_PLAN.md".to_string(),
            ],
            &root,
        )
        .expect("update project ingest policy");

        assert!(!output.contains("\"IMPLEMENTATION_PLAN.md\""));

        let inspected = run_in_dir(
            vec!["project".to_string(), "ingest-policy".to_string()],
            &root,
        )
        .expect("inspect updated ingest policy");
        assert!(!inspected.contains("\"IMPLEMENTATION_PLAN.md\""));
    }

    #[test]
    fn project_rerank_source_weights_update_persists_changes() {
        let root = temp_dir("colmem-cli-project-rerank-source-weights-update");
        let output = run_in_dir(
            vec![
                "project".to_string(),
                "rerank-source-weights".to_string(),
                "update".to_string(),
                "documentation_generic".to_string(),
                "33".to_string(),
            ],
            &root,
        )
        .expect("update rerank source weights");

        assert!(output.contains("\"documentation_generic\": 33"));

        let inspected = run_in_dir(
            vec!["project".to_string(), "rerank-source-weights".to_string()],
            &root,
        )
        .expect("inspect rerank source weights");
        assert!(inspected.contains("\"documentation_generic\": 33"));
    }

    #[test]
    fn ingest_and_query_are_testable_in_process() {
        let root = create_fixture_workspace();
        let ingest = run_in_dir(vec!["ingest".to_string()], &root).expect("ingest workspace");
        assert!(ingest.contains("Indexed project"));

        let query = run_in_dir(
            vec![
                "query".to_string(),
                "integration token".to_string(),
                "codex".to_string(),
                "builder".to_string(),
            ],
            &root,
        )
        .expect("query workspace");

        assert!(query.contains("integration token"));
        assert!(query.contains("src/lib.rs"));
    }

    #[test]
    fn facts_commands_are_testable_in_process() {
        let root = temp_dir("colmem-cli-facts");
        let add = run_in_dir(
            vec![
                "facts".to_string(),
                "add".to_string(),
                "colmem".to_string(),
                "supports".to_string(),
                "cli-tests".to_string(),
                "88".to_string(),
                "2026-04-10".to_string(),
            ],
            &root,
        )
        .expect("add fact");
        assert!(add.contains("\"supports\""));
        assert!(add.contains("\"decision\": \"create\""));

        let query = run_in_dir(
            vec![
                "facts".to_string(),
                "query".to_string(),
                "colmem supports cli-tests".to_string(),
                "active".to_string(),
                "2026-04-10".to_string(),
            ],
            &root,
        )
        .expect("query facts");
        assert!(query.contains("\"object\": \"cli-tests\""));

        let reinforce = run_in_dir(
            vec![
                "facts".to_string(),
                "add".to_string(),
                "colmem".to_string(),
                "supports".to_string(),
                "cli-tests".to_string(),
                "95".to_string(),
                "2026-04-11".to_string(),
            ],
            &root,
        )
        .expect("reinforce fact");
        assert!(reinforce.contains("\"decision\": \"reinforce\""));

        let bounded = run_in_dir(
            vec![
                "facts".to_string(),
                "add".to_string(),
                "colmem".to_string(),
                "supports".to_string(),
                "cli-tests".to_string(),
                "70".to_string(),
                "2026-04-12".to_string(),
                "2026-04-15".to_string(),
            ],
            &root,
        )
        .expect("bounded fact");
        assert!(bounded.contains("\"decision\": \"create\""));

        let invalidate_missing = run_in_dir(
            vec![
                "facts".to_string(),
                "invalidate".to_string(),
                "colmem".to_string(),
                "supports".to_string(),
                "missing".to_string(),
                "2026-04-12".to_string(),
            ],
            &root,
        )
        .expect("invalidate missing fact");
        assert!(invalidate_missing.contains("\"decision\": \"reject\""));
    }

    #[test]
    fn facts_summary_reports_backend_counts() {
        let root = temp_dir("colmem-cli-facts-summary");
        run_in_dir(
            vec![
                "facts".to_string(),
                "add".to_string(),
                "colmem".to_string(),
                "supports".to_string(),
                "fact summary".to_string(),
                "88".to_string(),
                "2026-04-13".to_string(),
            ],
            &root,
        )
        .expect("add fact");
        let summary = run_in_dir(
            vec![
                "facts".to_string(),
                "summary".to_string(),
                "2026-04-13".to_string(),
            ],
            &root,
        )
        .expect("fact summary");

        assert!(summary.contains("\"active\":"));
        assert!(summary.contains("\"audit_events\":"));
    }
}
