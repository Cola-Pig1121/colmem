use std::collections::{BTreeMap, BTreeSet};

use crate::agent::{AgentProfile, EvolutionPatch, EvolutionSignal};
use crate::capability::{
    BindingMode, CapabilityDescriptor, CapabilityPermission, CapabilityRegistry,
};
use crate::context::{ContextPack, ContextPackBuilder};
use crate::facts::{Fact, FactQueryScope, InMemoryFactStore};
use crate::host::HostContext;
use crate::model::{TaskKind, TransportKind};
use crate::project::ProjectScope;
use crate::record::IndexState;
use crate::retrieval::{HybridRetriever, QueryRequest, RetrievalPlan, SearchHit};
use crate::space::SpaceGraph;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug)]
pub struct TaskIntent {
    pub kind: TaskKind,
    pub summary: String,
    pub requested_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct CapabilitySelection {
    pub enabled: Vec<CapabilityDescriptor>,
    pub disabled: BTreeMap<String, String>,
    pub audit: Vec<CapabilityDecisionAudit>,
}

#[derive(Clone, Debug)]
pub struct CapabilityDecisionAudit {
    pub capability_id: String,
    pub outcome: String,
    pub binding_mode: BindingMode,
    pub project_required: bool,
    pub task_requested: bool,
    pub required_permissions: Vec<String>,
    pub reasons: Vec<String>,
}

impl CapabilityDecisionAudit {
    pub fn to_json(&self) -> String {
        json_object([
            ("capability_id".to_string(), quote(&self.capability_id)),
            ("outcome".to_string(), quote(&self.outcome)),
            (
                "binding_mode".to_string(),
                quote(match self.binding_mode {
                    BindingMode::Auto => "auto",
                    BindingMode::ForceEnabled => "force_enabled",
                    BindingMode::ForceDisabled => "force_disabled",
                }),
            ),
            (
                "project_required".to_string(),
                self.project_required.to_string(),
            ),
            (
                "task_requested".to_string(),
                self.task_requested.to_string(),
            ),
            (
                "required_permissions".to_string(),
                json_array(
                    self.required_permissions
                        .iter()
                        .map(|permission| quote(permission)),
                ),
            ),
            (
                "reasons".to_string(),
                json_array(self.reasons.iter().map(|reason| quote(reason))),
            ),
        ])
    }
}

impl CapabilitySelection {
    pub fn to_json(&self) -> String {
        json_object([
            (
                "enabled".to_string(),
                json_array(self.enabled.iter().map(CapabilityDescriptor::to_json)),
            ),
            (
                "disabled".to_string(),
                json_object(
                    self.disabled
                        .iter()
                        .map(|(id, reason)| (id.clone(), quote(reason))),
                ),
            ),
            (
                "audit".to_string(),
                json_array(self.audit.iter().map(CapabilityDecisionAudit::to_json)),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct HarnessSnapshot {
    pub selected_agent: String,
    pub selected_capabilities: CapabilitySelection,
    pub retrieval_plan: RetrievalPlan,
    pub fact_scope: FactQueryScope,
    pub fact_reference_date: String,
    pub fact_focus: bool,
    pub hits: Vec<SearchHit>,
    pub relevant_facts: Vec<Fact>,
    pub context_pack: ContextPack,
    pub evolution_preview: EvolutionPatch,
}

impl HarnessSnapshot {
    pub fn to_json(&self) -> String {
        json_object([
            ("selected_agent".to_string(), quote(&self.selected_agent)),
            (
                "selected_capabilities".to_string(),
                self.selected_capabilities.to_json(),
            ),
            ("retrieval_plan".to_string(), self.retrieval_plan.to_json()),
            (
                "fact_scope".to_string(),
                quote(match self.fact_scope {
                    FactQueryScope::Active => "active",
                    FactQueryScope::History => "history",
                    FactQueryScope::Scheduled => "scheduled",
                    FactQueryScope::All => "all",
                }),
            ),
            (
                "fact_reference_date".to_string(),
                quote(&self.fact_reference_date),
            ),
            ("fact_focus".to_string(), self.fact_focus.to_string()),
            (
                "hits".to_string(),
                json_array(self.hits.iter().map(SearchHit::to_json)),
            ),
            (
                "relevant_facts".to_string(),
                json_array(
                    self.relevant_facts
                        .iter()
                        .map(|fact| fact.to_json_with_status(&self.fact_reference_date)),
                ),
            ),
            ("context_pack".to_string(), self.context_pack.to_json()),
            (
                "evolution_preview".to_string(),
                evolution_patch_json(&self.evolution_preview),
            ),
        ])
    }
}

fn evolution_patch_json(patch: &EvolutionPatch) -> String {
    let persona = patch
        .persona
        .as_ref()
        .map(|shift| {
            json_object([
                (
                    "voice_override".to_string(),
                    shift
                        .voice_override
                        .as_ref()
                        .map(|voice| quote(voice))
                        .unwrap_or_else(|| "null".to_string()),
                ),
                (
                    "initiative_delta".to_string(),
                    shift.initiative_delta.to_string(),
                ),
                ("risk_delta".to_string(), shift.risk_delta.to_string()),
                (
                    "explanation_delta".to_string(),
                    shift.explanation_delta.to_string(),
                ),
            ])
        })
        .unwrap_or_else(|| "null".to_string());

    json_object([
        ("persona".to_string(), persona),
        (
            "skill_deltas".to_string(),
            json_object(
                patch
                    .skill_deltas
                    .iter()
                    .map(|(skill, delta)| (skill.clone(), delta.to_string())),
            ),
        ),
        (
            "preferred_capability_additions".to_string(),
            json_array(
                patch
                    .preferred_capability_additions
                    .iter()
                    .map(|id| quote(id)),
            ),
        ),
        (
            "watch_space_additions".to_string(),
            json_array(patch.watch_space_additions.iter().map(|space| quote(space))),
        ),
        (
            "memory_priority_deltas".to_string(),
            json_object(
                patch
                    .memory_priority_deltas
                    .iter()
                    .map(|(priority, delta)| (priority.clone(), delta.to_string())),
            ),
        ),
    ])
}

#[derive(Clone, Debug)]
pub struct HarnessRuntimeEngine {
    pub registry: CapabilityRegistry,
    pub graph: SpaceGraph,
    pub retriever: HybridRetriever,
    pub facts: InMemoryFactStore,
    pub index: IndexState,
    pub context_builder: ContextPackBuilder,
}

impl HarnessRuntimeEngine {
    fn permission_gate_reason(
        capability: &CapabilityDescriptor,
        host: &HostContext,
    ) -> Option<String> {
        if capability.stateful && !host.descriptor.supports_stateful_plugins {
            return Some("host disallows stateful capabilities".to_string());
        }

        for permission in capability.parsed_permissions() {
            match permission {
                CapabilityPermission::Read => {}
                CapabilityPermission::Write => {
                    if host.descriptor.transport != TransportKind::Cli {
                        return Some("write permission requires cli transport".to_string());
                    }
                }
                CapabilityPermission::Stdio => {
                    if host.descriptor.transport != TransportKind::StdioMcp {
                        return Some("stdio permission requires stdio mcp transport".to_string());
                    }
                }
                CapabilityPermission::Unknown(other) => {
                    return Some(format!("unknown permission requirement: {other}"));
                }
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn record_disabled(
        disabled: &mut BTreeMap<String, String>,
        audit: &mut Vec<CapabilityDecisionAudit>,
        capability: &CapabilityDescriptor,
        binding_mode: BindingMode,
        project_required: bool,
        task_requested: bool,
        reason: impl Into<String>,
        mut reasons: Vec<String>,
    ) {
        let reason = reason.into();
        disabled.insert(capability.id.clone(), reason.clone());
        reasons.push(format!("disabled: {reason}"));
        audit.push(CapabilityDecisionAudit {
            capability_id: capability.id.clone(),
            outcome: "disabled".to_string(),
            binding_mode,
            project_required,
            task_requested,
            required_permissions: capability.permissions.clone(),
            reasons,
        });
    }

    fn record_enabled(
        enabled: &mut Vec<CapabilityDescriptor>,
        audit: &mut Vec<CapabilityDecisionAudit>,
        capability: &CapabilityDescriptor,
        binding_mode: BindingMode,
        project_required: bool,
        task_requested: bool,
        mut reasons: Vec<String>,
    ) {
        if reasons.is_empty() {
            reasons.push("automatic capability match".to_string());
        } else {
            reasons.push("capability enabled".to_string());
        }
        enabled.push(capability.clone());
        audit.push(CapabilityDecisionAudit {
            capability_id: capability.id.clone(),
            outcome: "enabled".to_string(),
            binding_mode,
            project_required,
            task_requested,
            required_permissions: capability.permissions.clone(),
            reasons,
        });
    }

    fn tokenize(text: &str) -> Vec<String> {
        text.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .filter(|token| token.len() > 2)
            .map(|token| token.to_string())
            .collect()
    }

    fn overlap_score(text: &str, tokens: &[String]) -> usize {
        let haystack = text.to_ascii_lowercase();
        tokens
            .iter()
            .filter(|token| haystack.contains(token.as_str()))
            .count()
    }

    fn chunk_looks_like_test(chunk: &crate::record::Chunk) -> bool {
        let path = chunk.source_path.to_ascii_lowercase();
        let text = chunk.text.to_ascii_lowercase();
        path.contains("/tests/")
            || path.contains("\\tests\\")
            || text.contains("#[test]")
            || text.contains("assert_eq!(")
            || text.contains("assert!(")
            || text.contains("expect(\"")
            || (text.contains("fn ") && text.contains("test"))
    }

    fn annotate_hit_space_paths(&self, hits: &mut [SearchHit]) {
        for hit in hits {
            hit.space_path = self.graph.path_labels(&hit.space_id);
        }
    }

    fn chunk_to_hit(&self, chunk: &crate::record::Chunk, score: u8, reason: String) -> SearchHit {
        let space_id = chunk
            .space_ids
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| "workspace_root".to_string());
        SearchHit {
            chunk_id: chunk.id.clone(),
            space_path: chunk
                .space_paths
                .get(&space_id)
                .cloned()
                .unwrap_or_else(|| self.graph.path_labels(&space_id)),
            space_id,
            source_path: chunk.source_path.clone(),
            line_start: chunk.line_start,
            line_end: chunk.line_end,
            ordinal: chunk.ordinal,
            score,
            memory_path_match_count: 0,
            snippet: chunk.text.chars().take(220).collect(),
            evidence_ids: vec![chunk.id.clone(), chunk.record_id.clone()],
            reasons: vec![reason],
        }
    }

    fn resolve_evidence_ref(
        &self,
        evidence_ref: &str,
        fact: &Fact,
        limit: usize,
    ) -> Vec<SearchHit> {
        let fact_tokens = Self::tokenize(&format!(
            "{} {} {}",
            fact.subject, fact.predicate, fact.object
        ));
        let (kind, selector) = if let Some(value) = evidence_ref.strip_prefix("chunk:") {
            ("chunk", value)
        } else if let Some(value) = evidence_ref.strip_prefix("record:") {
            ("record", value)
        } else if let Some(value) = evidence_ref.strip_prefix("path:") {
            ("path", value)
        } else if evidence_ref.starts_with("chunk-") {
            ("chunk", evidence_ref)
        } else if evidence_ref.starts_with("record-") {
            ("record", evidence_ref)
        } else {
            return Vec::new();
        };

        match kind {
            "chunk" => self
                .index
                .chunks
                .iter()
                .find(|chunk| chunk.id == selector)
                .map(|chunk| {
                    vec![self.chunk_to_hit(
                        chunk,
                        98,
                        format!("fact evidence ref: chunk selector ({evidence_ref})"),
                    )]
                })
                .unwrap_or_default(),
            "record" => {
                let mut chunks = self
                    .index
                    .chunks
                    .iter()
                    .filter(|chunk| chunk.record_id == selector)
                    .map(|chunk| {
                        (
                            !Self::chunk_looks_like_test(chunk),
                            Self::overlap_score(&chunk.text, &fact_tokens),
                            chunk.ordinal,
                            chunk,
                        )
                    })
                    .collect::<Vec<_>>();
                chunks.sort_by(|left, right| {
                    right
                        .0
                        .cmp(&left.0)
                        .then_with(|| right.1.cmp(&left.1))
                        .then_with(|| left.2.cmp(&right.2))
                        .then_with(|| left.3.id.cmp(&right.3.id))
                });
                chunks
                    .into_iter()
                    .take(limit)
                    .map(|(_, _, _, chunk)| {
                        self.chunk_to_hit(
                            chunk,
                            95,
                            format!("fact evidence ref: record selector ({evidence_ref})"),
                        )
                    })
                    .collect()
            }
            "path" => {
                let mut chunks = self
                    .index
                    .chunks
                    .iter()
                    .filter(|chunk| chunk.source_path == selector)
                    .map(|chunk| {
                        (
                            !Self::chunk_looks_like_test(chunk),
                            Self::overlap_score(&chunk.text, &fact_tokens),
                            chunk.ordinal,
                            chunk,
                        )
                    })
                    .collect::<Vec<_>>();
                chunks.sort_by(|left, right| {
                    right
                        .0
                        .cmp(&left.0)
                        .then_with(|| right.1.cmp(&left.1))
                        .then_with(|| left.2.cmp(&right.2))
                        .then_with(|| left.3.id.cmp(&right.3.id))
                });
                chunks
                    .into_iter()
                    .take(limit)
                    .map(|(_, _, _, chunk)| {
                        self.chunk_to_hit(
                            chunk,
                            96,
                            format!("fact evidence ref: path selector ({evidence_ref})"),
                        )
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn fact_evidence_hits(
        &self,
        facts: &[Fact],
        fallback_hits: &[SearchHit],
        limit: usize,
    ) -> Vec<SearchHit> {
        let mut hits = Vec::new();
        let mut seen = BTreeSet::new();

        for fact in facts {
            for evidence_ref in &fact.evidence_ids {
                let remaining = limit.saturating_sub(hits.len());
                if remaining == 0 {
                    return hits;
                }
                for hit in self.resolve_evidence_ref(evidence_ref, fact, remaining) {
                    if seen.insert(hit.chunk_id.clone()) {
                        hits.push(hit);
                    }
                    if hits.len() >= limit {
                        return hits;
                    }
                }
            }
        }

        if !hits.is_empty() {
            return hits;
        }

        for hit in fallback_hits.iter().filter(|hit| {
            hit.reasons
                .iter()
                .any(|reason| reason.contains("fact alignment:"))
        }) {
            if seen.insert(hit.chunk_id.clone()) {
                hits.push(hit.clone());
            }
            if hits.len() >= limit {
                break;
            }
        }

        hits
    }

    pub fn select_capabilities(
        &self,
        agent: &AgentProfile,
        project: &ProjectScope,
        host: &HostContext,
        task: &TaskIntent,
    ) -> CapabilitySelection {
        let mut enabled = Vec::new();
        let mut disabled = BTreeMap::new();
        let mut audit = Vec::new();
        let host_disabled = project.disabled_for_host(host.host_id());
        let host_preferred = project.preferred_for_host(host.host_id());

        for capability in self.registry.list() {
            let manual = agent
                .manual_capability_modes
                .get(&capability.id)
                .cloned()
                .unwrap_or(BindingMode::Auto);
            let project_required = project.required_capabilities.contains(&capability.id);
            let task_requested = task.requested_capabilities.contains(&capability.id);
            let mut reasons = Vec::new();

            if project_required {
                reasons.push("project requirement".to_string());
            }
            if task_requested {
                reasons.push("task requested capability".to_string());
            }
            if agent
                .skill_profile
                .preferred_capabilities
                .contains(&capability.id)
            {
                reasons.push("agent preferred capability".to_string());
            }
            if host_preferred.contains(&capability.id) {
                reasons.push("host preferred capability".to_string());
            }
            if manual == BindingMode::ForceEnabled {
                reasons.push("agent override: force enabled".to_string());
            }

            if manual == BindingMode::ForceDisabled {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "agent override",
                    reasons,
                );
                continue;
            }
            if host_disabled.contains(&capability.id) {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "project override",
                    reasons,
                );
                continue;
            }
            if !host.supports_kind(&capability.kind) {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "host incompatibility",
                    reasons,
                );
                continue;
            }
            if !capability.matches_host(host.host_id()) {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "capability host filter",
                    reasons,
                );
                continue;
            }
            if let Some(reason) = Self::permission_gate_reason(capability, host) {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    reason,
                    reasons,
                );
                continue;
            }
            if manual != BindingMode::ForceEnabled && !capability.matches_role(&agent.role) {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "agent role mismatch",
                    reasons,
                );
                continue;
            }
            if manual != BindingMode::ForceEnabled
                && !task_requested
                && !capability.matches_task(&task.kind)
            {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "task hint mismatch",
                    reasons,
                );
                continue;
            }
            if manual != BindingMode::ForceEnabled
                && !project_required
                && !capability.matches_project(project)
            {
                Self::record_disabled(
                    &mut disabled,
                    &mut audit,
                    capability,
                    manual,
                    project_required,
                    task_requested,
                    "project tags mismatch",
                    reasons,
                );
                continue;
            }

            Self::record_enabled(
                &mut enabled,
                &mut audit,
                capability,
                manual,
                project_required,
                task_requested,
                reasons,
            );
        }

        enabled.sort_by(|left, right| {
            let left_preferred = agent
                .skill_profile
                .preferred_capabilities
                .contains(&left.id)
                || host_preferred.contains(&left.id);
            let right_preferred = agent
                .skill_profile
                .preferred_capabilities
                .contains(&right.id)
                || host_preferred.contains(&right.id);
            right_preferred
                .cmp(&left_preferred)
                .then_with(|| left.id.cmp(&right.id))
        });
        enabled.dedup_by(|left, right| left.id == right.id);

        CapabilitySelection {
            enabled,
            disabled,
            audit,
        }
    }

    pub fn prepare_run(
        &self,
        agent: &AgentProfile,
        project: &ProjectScope,
        host: &HostContext,
        task: &TaskIntent,
    ) -> HarnessSnapshot {
        let reference_date = InMemoryFactStore::today_iso_utc();
        self.prepare_run_with_fact_scope(
            agent,
            project,
            host,
            task,
            FactQueryScope::All,
            &reference_date,
        )
    }

    pub fn prepare_run_with_fact_scope(
        &self,
        agent: &AgentProfile,
        project: &ProjectScope,
        host: &HostContext,
        task: &TaskIntent,
        fact_scope: FactQueryScope,
        reference_date: &str,
    ) -> HarnessSnapshot {
        let capability_selection = self.select_capabilities(agent, project, host, task);
        let request = QueryRequest {
            text: task.summary.clone(),
            project_id: project.id.clone(),
            task_kind: task.kind.clone(),
            seed_space: agent.habitat.watch_spaces.iter().next().cloned(),
        };
        let mut retriever = self.retriever.clone();
        retriever.reranker.policy.source_weights = project.rerank_source_weights.clone();
        let mut retrieval_plan = retriever.plan(&self.graph, project, agent, host, &request);
        let relevant_facts =
            self.facts
                .facts_for_query_scoped(&task.summary, fact_scope, reference_date);
        let fact_focus = !relevant_facts.is_empty()
            && self
                .facts
                .facts_for_query_scoped(&task.summary, fact_scope, reference_date)
                .first()
                .map(|_| self.facts.best_match_score(&task.summary).unwrap_or(0) >= 2)
                .unwrap_or(false);
        if fact_focus {
            retrieval_plan
                .notes
                .push("presentation=fact_first".to_string());
        }
        retrieval_plan.notes.push(format!(
            "fact_scope={}",
            match fact_scope {
                FactQueryScope::Active => "active",
                FactQueryScope::History => "history",
                FactQueryScope::Scheduled => "scheduled",
                FactQueryScope::All => "all",
            }
        ));
        retrieval_plan
            .notes
            .push(format!("fact_reference_date={reference_date}"));
        let fact_hints =
            self.facts
                .rerank_hints_for_query_scoped(&task.summary, fact_scope, reference_date);
        let mut hits = if self.index.chunks.is_empty() {
            retriever.demo_hits(&request, &retrieval_plan)
        } else {
            retriever.index_hits(&self.index, &request, &retrieval_plan, &fact_hints, 5)
        };
        self.annotate_hit_space_paths(&mut hits);
        let fact_evidence_hits = self.fact_evidence_hits(&relevant_facts, &hits, 3);
        let context_hits = if fact_focus && !fact_evidence_hits.is_empty() {
            fact_evidence_hits
        } else {
            hits.clone()
        };
        let context_pack = self.context_builder.build(
            agent,
            &project.id,
            &context_hits,
            &relevant_facts,
            fact_focus,
        );
        let evolution_preview = EvolutionPatch::from_signal(&EvolutionSignal {
            successful_capabilities: capability_selection
                .enabled
                .iter()
                .take(2)
                .map(|capability| capability.id.clone())
                .collect(),
            failed_capabilities: BTreeSet::new(),
            promoted_skills: agent
                .skill_profile
                .domains
                .keys()
                .take(1)
                .cloned()
                .collect(),
            discouraged_skills: BTreeSet::new(),
            watch_space_additions: retrieval_plan
                .candidate_spaces
                .iter()
                .take(1)
                .cloned()
                .collect(),
            persona_shift: Default::default(),
        });

        HarnessSnapshot {
            selected_agent: agent.id.clone(),
            selected_capabilities: capability_selection,
            retrieval_plan,
            fact_scope,
            fact_reference_date: reference_date.to_string(),
            fact_focus,
            hits,
            relevant_facts,
            context_pack,
            evolution_preview,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::agent::{AgentHabitat, AgentProfile, PersonaProfile, SkillProfile};
    use crate::capability::{BindingMode, CapabilityDescriptor, CapabilityRegistry};
    use crate::context::ContextPackBuilder;
    use crate::facts::{Fact, FactQueryScope, InMemoryFactStore};
    use crate::host::{HostContext, HostDescriptor};
    use crate::model::{AgentRole, CapabilityKind, HostId, TaskKind, TransportKind};
    use crate::project::ProjectScope;
    use crate::record::{Chunk, ChunkSourceKind, FullTextIndex, IndexState, TokenPosting};
    use crate::retrieval::HybridRetriever;
    use crate::space::{SpaceGraph, SpaceLink, SpaceLinkKind, SpaceNode};

    use super::{HarnessRuntimeEngine, TaskIntent};

    fn test_agent() -> AgentProfile {
        let mut domains = BTreeMap::new();
        domains.insert("rust".to_string(), 80);
        AgentProfile {
            id: "builder".to_string(),
            display_name: "Builder".to_string(),
            role: AgentRole::Builder,
            mission: "Ship Rust runtime code".to_string(),
            persona: PersonaProfile {
                voice: "direct".to_string(),
                initiative: 70,
                risk_appetite: 40,
                explanation_depth: 55,
            },
            habitat: AgentHabitat {
                home_space: "architecture".to_string(),
                accessible_spaces: BTreeSet::from([
                    "retrieval".to_string(),
                    "agent_runtime".to_string(),
                ]),
                watch_spaces: BTreeSet::from(["retrieval".to_string()]),
            },
            skill_profile: SkillProfile {
                domains,
                preferred_capabilities: BTreeSet::from(["repo_search".to_string()]),
            },
            memory_priorities: BTreeMap::new(),
            manual_capability_modes: BTreeMap::from([(
                "cursor_plugin".to_string(),
                BindingMode::ForceDisabled,
            )]),
        }
    }

    fn test_registry() -> CapabilityRegistry {
        let mut registry = CapabilityRegistry::default();
        registry.register(CapabilityDescriptor {
            id: "repo_search".to_string(),
            kind: CapabilityKind::Tool,
            provider: "builtin".to_string(),
            version: "0.1.0".to_string(),
            summary: "search project data".to_string(),
            compatible_hosts: BTreeSet::new(),
            compatible_roles: BTreeSet::from([AgentRole::Builder]),
            project_tags: BTreeSet::from(["rust".to_string()]),
            permissions: vec!["read".to_string()],
            activation_hints: BTreeSet::from([TaskKind::Query, TaskKind::Refactor]),
            stateful: false,
        });
        registry.register(CapabilityDescriptor {
            id: "cursor_plugin".to_string(),
            kind: CapabilityKind::Plugin,
            provider: "builtin".to_string(),
            version: "0.1.0".to_string(),
            summary: "cursor integration".to_string(),
            compatible_hosts: BTreeSet::from([HostId::Cursor]),
            compatible_roles: BTreeSet::from([AgentRole::Builder]),
            project_tags: BTreeSet::new(),
            permissions: vec!["read".to_string()],
            activation_hints: BTreeSet::new(),
            stateful: true,
        });
        registry.register(CapabilityDescriptor {
            id: "rust_refactor".to_string(),
            kind: CapabilityKind::Skill,
            provider: "builtin".to_string(),
            version: "0.1.0".to_string(),
            summary: "apply write-capable Rust refactors".to_string(),
            compatible_hosts: BTreeSet::new(),
            compatible_roles: BTreeSet::from([AgentRole::Builder]),
            project_tags: BTreeSet::from(["rust".to_string()]),
            permissions: vec!["read".to_string(), "write".to_string()],
            activation_hints: BTreeSet::from([TaskKind::Refactor]),
            stateful: false,
        });
        registry
    }

    fn test_graph() -> SpaceGraph {
        let mut graph = SpaceGraph::default();
        graph.add_node(SpaceNode {
            id: "architecture".to_string(),
            label: "Architecture".to_string(),
            parent_id: None,
            tags: BTreeSet::new(),
        });
        graph.add_node(SpaceNode {
            id: "retrieval".to_string(),
            label: "Retrieval".to_string(),
            parent_id: Some("architecture".to_string()),
            tags: BTreeSet::new(),
        });
        graph.add_node(SpaceNode {
            id: "agent_runtime".to_string(),
            label: "Agent Runtime".to_string(),
            parent_id: Some("architecture".to_string()),
            tags: BTreeSet::new(),
        });
        graph.add_link(SpaceLink {
            from: "retrieval".to_string(),
            to: "agent_runtime".to_string(),
            kind: SpaceLinkKind::DependsOn,
            weight: 80,
        });
        graph
    }

    fn test_project() -> ProjectScope {
        let mut project =
            ProjectScope::new("colmem", "Colmem", "D:/Code/Mempalace/mempalace/colmem");
        project.tags.insert("rust".to_string());
        project.focus_spaces.insert("architecture".to_string());
        project
    }

    fn test_host() -> HostContext {
        HostContext::new(HostDescriptor {
            id: HostId::Codex,
            display_name: "Codex",
            transport: TransportKind::Cli,
            supports_stateful_plugins: true,
            supported_capability_kinds: BTreeSet::from([
                CapabilityKind::Skill,
                CapabilityKind::Tool,
                CapabilityKind::Plugin,
                CapabilityKind::McpEndpoint,
            ]),
            install_hint: "Use the colmem CLI or MCP transport.",
        })
    }

    fn test_stdio_host_without_stateful_plugins() -> HostContext {
        HostContext::new(HostDescriptor {
            id: HostId::Cursor,
            display_name: "Cursor MCP",
            transport: TransportKind::StdioMcp,
            supports_stateful_plugins: false,
            supported_capability_kinds: BTreeSet::from([
                CapabilityKind::Skill,
                CapabilityKind::Tool,
                CapabilityKind::Plugin,
                CapabilityKind::McpEndpoint,
            ]),
            install_hint: "Use stdio MCP transport.",
        })
    }

    #[test]
    fn manual_disable_beats_auto_selection() {
        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts: InMemoryFactStore::default(),
            index: IndexState::default(),
            context_builder: ContextPackBuilder::default(),
        };
        let selection = engine.select_capabilities(
            &test_agent(),
            &test_project(),
            &test_host(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "plan the retrieval runtime".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        assert!(
            selection
                .enabled
                .iter()
                .any(|capability| capability.id == "repo_search")
        );
        assert_eq!(
            selection.disabled.get("cursor_plugin"),
            Some(&"agent override".to_string())
        );
        assert!(
            selection
                .audit
                .iter()
                .any(|entry| entry.capability_id == "repo_search" && entry.outcome == "enabled")
        );
    }

    #[test]
    fn write_permission_is_enforced_for_stdio_hosts() {
        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts: InMemoryFactStore::default(),
            index: IndexState::default(),
            context_builder: ContextPackBuilder::default(),
        };

        let selection = engine.select_capabilities(
            &test_agent(),
            &test_project(),
            &test_stdio_host_without_stateful_plugins(),
            &TaskIntent {
                kind: TaskKind::Refactor,
                summary: "perform a rust refactor".to_string(),
                requested_capabilities: BTreeSet::from(["rust_refactor".to_string()]),
            },
        );

        assert_eq!(
            selection.disabled.get("rust_refactor"),
            Some(&"write permission requires cli transport".to_string())
        );
        assert!(selection.audit.iter().any(|entry| {
            entry.capability_id == "rust_refactor"
                && entry.outcome == "disabled"
                && entry
                    .reasons
                    .iter()
                    .any(|reason| reason.contains("write permission requires cli transport"))
        }));
    }

    #[test]
    fn stateful_capabilities_do_not_bypass_host_safety_rules() {
        let mut agent = test_agent();
        agent
            .manual_capability_modes
            .insert("cursor_plugin".to_string(), BindingMode::ForceEnabled);
        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts: InMemoryFactStore::default(),
            index: IndexState::default(),
            context_builder: ContextPackBuilder::default(),
        };

        let selection = engine.select_capabilities(
            &agent,
            &test_project(),
            &test_stdio_host_without_stateful_plugins(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "inspect host safety".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        assert_eq!(
            selection.disabled.get("cursor_plugin"),
            Some(&"host disallows stateful capabilities".to_string())
        );
        assert!(selection.audit.iter().any(|entry| {
            entry.capability_id == "cursor_plugin"
                && entry.binding_mode == BindingMode::ForceEnabled
                && entry
                    .reasons
                    .iter()
                    .any(|reason| reason.contains("host disallows stateful capabilities"))
        }));
    }

    #[test]
    fn prepare_run_builds_context_and_hits() {
        let mut facts = InMemoryFactStore::default();
        facts.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "uses".to_string(),
            object: "hybrid retrieval".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 90,
            evidence_ids: vec!["fact-1".to_string()],
        });

        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts,
            index: IndexState::default(),
            context_builder: ContextPackBuilder::default(),
        };
        let snapshot = engine.prepare_run(
            &test_agent(),
            &test_project(),
            &test_host(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "hybrid retrieval design".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        assert!(!snapshot.hits.is_empty());
        assert!(!snapshot.context_pack.sections.is_empty());
        assert_eq!(snapshot.relevant_facts.len(), 1);
        assert!(snapshot.fact_focus);
        assert_eq!(snapshot.fact_scope, FactQueryScope::All);
        assert_eq!(
            snapshot
                .context_pack
                .sections
                .first()
                .map(|section| section.title.as_str()),
            Some("Fact Matches")
        );
    }

    #[test]
    fn non_fact_query_keeps_memory_mapped_evidence_in_context() {
        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts: InMemoryFactStore::default(),
            index: IndexState::default(),
            context_builder: ContextPackBuilder::default(),
        };
        let snapshot = engine.prepare_run(
            &test_agent(),
            &test_project(),
            &test_host(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "retrieval architecture".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        let evidence = snapshot
            .context_pack
            .sections
            .iter()
            .find(|section| section.title == "Evidence")
            .expect("evidence section");
        assert!(!evidence.entries.is_empty());
        assert!(!snapshot.context_pack.memory_map.is_empty());
        assert!(
            snapshot
                .context_pack
                .memory_map
                .iter()
                .any(|entry| entry.memory_path.contains("Architecture"))
        );
    }

    #[test]
    fn project_rerank_source_weights_influence_harness_ordering() {
        let mut project = test_project();
        project.rerank_source_weights.implementation_default = -20;
        project.rerank_source_weights.documentation_generic = 30;
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::from([(
                    "runtime".to_string(),
                    vec![
                        TokenPosting {
                            chunk_id: "impl-runtime".to_string(),
                            frequency: 1,
                        },
                        TokenPosting {
                            chunk_id: "docs-runtime".to_string(),
                            frequency: 1,
                        },
                    ],
                )]),
            },
            records: Vec::new(),
            chunks: vec![
                Chunk {
                    id: "impl-runtime".to_string(),
                    record_id: "record-impl".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/runtime.rs".to_string(),
                    source_kind: ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 1,
                    char_count: 32,
                    text: "runtime execution model".to_string(),
                    space_ids: BTreeSet::from(["agent_runtime".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "impl".to_string(),
                },
                Chunk {
                    id: "docs-runtime".to_string(),
                    record_id: "record-docs".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "docs/runtime.md".to_string(),
                    source_kind: ChunkSourceKind::Documentation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 1,
                    char_count: 32,
                    text: "runtime execution model".to_string(),
                    space_ids: BTreeSet::from(["architecture".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "docs".to_string(),
                },
            ],
            ..Default::default()
        };
        let engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts: InMemoryFactStore::default(),
            index,
            context_builder: ContextPackBuilder::default(),
        };
        let snapshot = engine.prepare_run(
            &test_agent(),
            &project,
            &test_host(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "runtime".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        assert_eq!(
            snapshot.hits.first().map(|hit| hit.chunk_id.as_str()),
            Some("docs-runtime")
        );
    }

    #[test]
    fn explicit_fact_path_evidence_is_used_before_fallback_hits() {
        let mut facts = InMemoryFactStore::default();
        facts.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 90,
            evidence_ids: vec!["path:src/mcp.rs".to_string()],
        });
        let index = IndexState {
            version: 1,
            ..Default::default()
        };
        let mut engine = HarnessRuntimeEngine {
            registry: test_registry(),
            graph: test_graph(),
            retriever: HybridRetriever::default(),
            facts,
            index,
            context_builder: ContextPackBuilder::default(),
        };
        engine.index.chunks.push(crate::record::Chunk {
            id: "chunk-mcp".to_string(),
            record_id: "record-mcp".to_string(),
            project_id: "colmem".to_string(),
            source_path: "src/mcp.rs".to_string(),
            source_kind: crate::record::ChunkSourceKind::Implementation,
            ordinal: 0,
            line_start: 1,
            line_end: 3,
            char_count: 24,
            text: "colmem mcp runtime support".to_string(),
            space_ids: BTreeSet::from(["agent_runtime".to_string()]),
            space_paths: BTreeMap::new(),
            hash: "hash".to_string(),
        });

        let snapshot = engine.prepare_run(
            &test_agent(),
            &test_project(),
            &test_host(),
            &TaskIntent {
                kind: TaskKind::Query,
                summary: "colmem supports mcp".to_string(),
                requested_capabilities: BTreeSet::new(),
            },
        );

        let fact_evidence = snapshot
            .context_pack
            .sections
            .iter()
            .find(|section| section.title == "Fact Evidence")
            .expect("fact evidence");
        assert!(!fact_evidence.entries.is_empty());
        assert!(
            fact_evidence
                .entries
                .iter()
                .all(|entry| entry.contains("src/mcp.rs"))
        );
        assert!(
            fact_evidence
                .entries
                .iter()
                .any(|entry| entry.contains("Architecture > Agent Runtime"))
        );
    }
}
