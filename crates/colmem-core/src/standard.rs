use std::collections::{BTreeMap, BTreeSet};

use crate::agent::{AgentHabitat, AgentProfile, PersonaProfile, SkillProfile};
use crate::capability::{CapabilityDescriptor, CapabilityRegistry};
use crate::context::ContextPackBuilder;
use crate::facts::{Fact, InMemoryFactStore};
use crate::harness::HarnessRuntimeEngine;
use crate::model::{AgentRole, CapabilityKind, HostId, TaskKind};
use crate::project::{ProjectHostPolicy, ProjectScope};
use crate::record::IndexState;
use crate::retrieval::HybridRetriever;
use crate::space::{SpaceGraph, SpaceLink, SpaceLinkKind, SpaceNode};

pub fn standard_registry() -> CapabilityRegistry {
    let mut registry = CapabilityRegistry::default();
    registry.register(CapabilityDescriptor {
        id: "repo_search".to_string(),
        kind: CapabilityKind::Tool,
        provider: "colmem".to_string(),
        version: "0.1.0".to_string(),
        summary: "Search indexed project and memory records.".to_string(),
        compatible_hosts: BTreeSet::new(),
        compatible_roles: BTreeSet::from([AgentRole::Builder, AgentRole::Researcher]),
        project_tags: BTreeSet::new(),
        permissions: vec!["read".to_string()],
        activation_hints: BTreeSet::from([TaskKind::Query, TaskKind::Review, TaskKind::Refactor]),
        stateful: false,
    });
    registry.register(CapabilityDescriptor {
        id: "rust_refactor".to_string(),
        kind: CapabilityKind::Skill,
        provider: "colmem".to_string(),
        version: "0.1.0".to_string(),
        summary: "Apply Rust-first architecture and refactor heuristics.".to_string(),
        compatible_hosts: BTreeSet::new(),
        compatible_roles: BTreeSet::from([AgentRole::Builder, AgentRole::Architect]),
        project_tags: BTreeSet::from(["rust".to_string()]),
        permissions: vec!["read".to_string(), "write".to_string()],
        activation_hints: BTreeSet::from([TaskKind::Refactor]),
        stateful: false,
    });
    registry.register(CapabilityDescriptor {
        id: "mcp_bridge".to_string(),
        kind: CapabilityKind::McpEndpoint,
        provider: "colmem".to_string(),
        version: "0.1.0".to_string(),
        summary: "Expose runtime state over the Model Context Protocol.".to_string(),
        compatible_hosts: BTreeSet::new(),
        compatible_roles: BTreeSet::new(),
        project_tags: BTreeSet::new(),
        permissions: vec!["stdio".to_string()],
        activation_hints: BTreeSet::from([TaskKind::Serve]),
        stateful: true,
    });
    registry.register(CapabilityDescriptor {
        id: "cursor_plugin".to_string(),
        kind: CapabilityKind::Plugin,
        provider: "colmem".to_string(),
        version: "0.1.0".to_string(),
        summary: "Cursor-side plugin bridge.".to_string(),
        compatible_hosts: BTreeSet::from([HostId::Cursor]),
        compatible_roles: BTreeSet::new(),
        project_tags: BTreeSet::new(),
        permissions: vec!["read".to_string()],
        activation_hints: BTreeSet::new(),
        stateful: true,
    });
    registry
}

pub fn standard_space_graph() -> SpaceGraph {
    let mut graph = SpaceGraph::default();
    for (id, label, parent) in [
        ("workspace_root", "Workspace Root", None),
        ("architecture", "Architecture", Some("workspace_root")),
        ("retrieval", "Retrieval", Some("architecture")),
        ("agent_runtime", "Agent Runtime", Some("architecture")),
        ("host_adapters", "Host Adapters", Some("workspace_root")),
        ("facts", "Fact Store", Some("workspace_root")),
    ] {
        graph.add_node(SpaceNode {
            id: id.to_string(),
            label: label.to_string(),
            parent_id: parent.map(|value| value.to_string()),
            tags: BTreeSet::new(),
        });
    }
    graph.add_link(SpaceLink {
        from: "retrieval".to_string(),
        to: "facts".to_string(),
        kind: SpaceLinkKind::SharedEntity,
        weight: 90,
    });
    graph.add_link(SpaceLink {
        from: "agent_runtime".to_string(),
        to: "retrieval".to_string(),
        kind: SpaceLinkKind::DependsOn,
        weight: 90,
    });
    graph.add_link(SpaceLink {
        from: "host_adapters".to_string(),
        to: "agent_runtime".to_string(),
        kind: SpaceLinkKind::References,
        weight: 75,
    });
    graph
}

pub fn standard_agents() -> Vec<AgentProfile> {
    let mut builder_domains = BTreeMap::new();
    builder_domains.insert("rust".to_string(), 85);
    builder_domains.insert("mcp".to_string(), 72);

    let mut architect_domains = BTreeMap::new();
    architect_domains.insert("architecture".to_string(), 92);
    architect_domains.insert("retrieval".to_string(), 78);

    vec![
        AgentProfile {
            id: "builder".to_string(),
            display_name: "Builder".to_string(),
            role: AgentRole::Builder,
            mission: "Implement host-aware runtime code without losing architectural clarity."
                .to_string(),
            persona: PersonaProfile {
                voice: "direct".to_string(),
                initiative: 72,
                risk_appetite: 44,
                explanation_depth: 48,
            },
            habitat: AgentHabitat {
                home_space: "agent_runtime".to_string(),
                accessible_spaces: BTreeSet::from([
                    "retrieval".to_string(),
                    "host_adapters".to_string(),
                ]),
                watch_spaces: BTreeSet::from(["retrieval".to_string()]),
            },
            skill_profile: SkillProfile {
                domains: builder_domains,
                preferred_capabilities: BTreeSet::from([
                    "repo_search".to_string(),
                    "rust_refactor".to_string(),
                ]),
            },
            memory_priorities: BTreeMap::from([
                ("evidence".to_string(), 90),
                ("facts".to_string(), 74),
            ]),
            manual_capability_modes: BTreeMap::new(),
        },
        AgentProfile {
            id: "architect".to_string(),
            display_name: "Architect".to_string(),
            role: AgentRole::Architect,
            mission: "Evolve system structure, terminology, and policy boundaries.".to_string(),
            persona: PersonaProfile {
                voice: "systematic".to_string(),
                initiative: 68,
                risk_appetite: 30,
                explanation_depth: 82,
            },
            habitat: AgentHabitat {
                home_space: "architecture".to_string(),
                accessible_spaces: BTreeSet::from(["retrieval".to_string(), "facts".to_string()]),
                watch_spaces: BTreeSet::from(["architecture".to_string()]),
            },
            skill_profile: SkillProfile {
                domains: architect_domains,
                preferred_capabilities: BTreeSet::from(["rust_refactor".to_string()]),
            },
            memory_priorities: BTreeMap::from([
                ("design".to_string(), 92),
                ("constraints".to_string(), 88),
            ]),
            manual_capability_modes: BTreeMap::new(),
        },
    ]
}

pub fn standard_project(root_path: impl Into<String>) -> ProjectScope {
    let mut project = ProjectScope::new("colmem", "Colmem", root_path);
    project.tags = BTreeSet::from(["rust".to_string(), "mcp".to_string(), "cli".to_string()]);
    project.focus_spaces = BTreeSet::from([
        "architecture".to_string(),
        "retrieval".to_string(),
        "agent_runtime".to_string(),
    ]);
    project.required_capabilities = BTreeSet::from(["repo_search".to_string()]);
    project.host_overrides.insert(
        HostId::Cursor,
        ProjectHostPolicy {
            disabled_capabilities: BTreeSet::new(),
            preferred_capabilities: BTreeSet::from(["cursor_plugin".to_string()]),
        },
    );
    project
}

pub fn standard_fact_store() -> InMemoryFactStore {
    let mut store = InMemoryFactStore::default();
    store.add_fact(Fact {
        subject: "colmem".to_string(),
        predicate: "replaces".to_string(),
        object: "sdk-specific runtime bindings".to_string(),
        valid_from: Some("2026-04-09".to_string()),
        valid_to: None,
        confidence: 96,
        evidence_ids: vec!["path:crates/colmem-core/src/standard.rs".to_string()],
    });
    store.add_fact(Fact {
        subject: "colmem".to_string(),
        predicate: "prefers".to_string(),
        object: "hybrid retrieval".to_string(),
        valid_from: Some("2026-04-09".to_string()),
        valid_to: None,
        confidence: 93,
        evidence_ids: vec!["path:crates/colmem-core/src/retrieval.rs".to_string()],
    });
    store
}

pub fn standard_harness() -> HarnessRuntimeEngine {
    HarnessRuntimeEngine {
        registry: standard_registry(),
        graph: standard_space_graph(),
        retriever: HybridRetriever::default(),
        facts: standard_fact_store(),
        index: IndexState::default(),
        context_builder: ContextPackBuilder::default(),
    }
}
