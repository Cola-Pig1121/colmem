use crate::agent::AgentProfile;
use crate::facts::Fact;
use crate::retrieval::SearchHit;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug)]
pub struct MemoryMapEntry {
    pub space_id: String,
    pub memory_path: String,
    pub evidence_count: usize,
    pub top_sources: Vec<String>,
}

impl MemoryMapEntry {
    pub fn to_json(&self) -> String {
        json_object([
            ("space_id".to_string(), quote(&self.space_id)),
            ("memory_path".to_string(), quote(&self.memory_path)),
            (
                "evidence_count".to_string(),
                self.evidence_count.to_string(),
            ),
            (
                "top_sources".to_string(),
                json_array(self.top_sources.iter().map(|source| quote(source))),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct ContextSection {
    pub title: String,
    pub entries: Vec<String>,
}

impl ContextSection {
    pub fn to_json(&self) -> String {
        json_object([
            ("title".to_string(), quote(&self.title)),
            (
                "entries".to_string(),
                json_array(self.entries.iter().map(|entry| quote(entry))),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct ContextPack {
    pub agent_id: String,
    pub project_id: String,
    pub sections: Vec<ContextSection>,
    pub memory_map: Vec<MemoryMapEntry>,
    pub citations: Vec<String>,
    pub policies: Vec<String>,
}

impl ContextPack {
    pub fn to_json(&self) -> String {
        json_object([
            ("agent_id".to_string(), quote(&self.agent_id)),
            ("project_id".to_string(), quote(&self.project_id)),
            (
                "sections".to_string(),
                json_array(self.sections.iter().map(ContextSection::to_json)),
            ),
            (
                "memory_map".to_string(),
                json_array(self.memory_map.iter().map(MemoryMapEntry::to_json)),
            ),
            (
                "citations".to_string(),
                json_array(self.citations.iter().map(|citation| quote(citation))),
            ),
            (
                "policies".to_string(),
                json_array(self.policies.iter().map(|policy| quote(policy))),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct ContextPackBuilder {
    pub max_evidence: usize,
}

impl Default for ContextPackBuilder {
    fn default() -> Self {
        Self { max_evidence: 3 }
    }
}

impl ContextPackBuilder {
    fn memory_map(&self, hits: &[SearchHit]) -> Vec<MemoryMapEntry> {
        let mut entries = Vec::<MemoryMapEntry>::new();
        for hit in hits.iter().take(self.max_evidence) {
            let memory_path = hit.memory_path();
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| entry.space_id == hit.space_id)
            {
                entry.evidence_count += 1;
                if !entry.top_sources.contains(&hit.source_path) {
                    entry.top_sources.push(hit.source_path.clone());
                }
            } else {
                entries.push(MemoryMapEntry {
                    space_id: hit.space_id.clone(),
                    memory_path,
                    evidence_count: 1,
                    top_sources: vec![hit.source_path.clone()],
                });
            }
        }
        entries
    }

    fn fact_entry(&self, fact: &Fact) -> String {
        let status = match (&fact.valid_from, &fact.valid_to) {
            (Some(valid_from), Some(valid_to)) => format!("valid={valid_from}..{valid_to}"),
            (Some(valid_from), None) => format!("valid_from={valid_from}"),
            (None, Some(valid_to)) => format!("valid_until={valid_to}"),
            (None, None) => "validity=unspecified".to_string(),
        };
        let evidence = if fact.evidence_ids.is_empty() {
            "evidence=none".to_string()
        } else {
            format!("evidence={}", fact.evidence_ids.join(","))
        };
        format!(
            "{} {} {} [confidence={}, {}, {}]",
            fact.subject, fact.predicate, fact.object, fact.confidence, status, evidence
        )
    }

    pub fn build(
        &self,
        agent: &AgentProfile,
        project_id: &str,
        hits: &[SearchHit],
        facts: &[Fact],
        fact_focus: bool,
    ) -> ContextPack {
        let evidence_entries = hits
            .iter()
            .take(self.max_evidence)
            .map(|hit| {
                format!(
                    "{} [{}] {}:{}-{} {}",
                    hit.memory_path(),
                    hit.score,
                    hit.source_path,
                    hit.line_start,
                    hit.line_end,
                    hit.snippet
                )
            })
            .collect::<Vec<_>>();
        let fact_entries = facts
            .iter()
            .take(self.max_evidence)
            .map(|fact| self.fact_entry(fact))
            .collect::<Vec<_>>();
        let fact_evidence_entries = hits
            .iter()
            .take(self.max_evidence)
            .map(|hit| {
                format!(
                    "{} [{}] {}:{}-{} {}",
                    hit.memory_path(),
                    hit.score,
                    hit.source_path,
                    hit.line_start,
                    hit.line_end,
                    hit.snippet
                )
            })
            .collect::<Vec<_>>();

        let mut citations = hits
            .iter()
            .flat_map(|hit| hit.evidence_ids.iter().cloned())
            .take(self.max_evidence)
            .collect::<Vec<_>>();
        citations.extend(
            facts
                .iter()
                .flat_map(|fact| fact.evidence_ids.iter().cloned())
                .take(self.max_evidence),
        );

        let mut sections = Vec::new();
        let memory_map = self.memory_map(hits);
        if fact_focus && !fact_entries.is_empty() {
            sections.push(ContextSection {
                title: "Fact Matches".to_string(),
                entries: fact_entries.clone(),
            });
            if !fact_evidence_entries.is_empty() {
                sections.push(ContextSection {
                    title: "Fact Evidence".to_string(),
                    entries: fact_evidence_entries,
                });
            }
        }
        sections.push(ContextSection {
            title: "Agent Persona".to_string(),
            entries: vec![
                format!("voice={}", agent.persona.voice),
                format!("initiative={}", agent.persona.initiative),
                format!("risk_appetite={}", agent.persona.risk_appetite),
            ],
        });
        sections.push(ContextSection {
            title: "Evidence".to_string(),
            entries: evidence_entries,
        });
        if !fact_focus {
            sections.push(ContextSection {
                title: "Facts".to_string(),
                entries: fact_entries,
            });
        }

        ContextPack {
            agent_id: agent.id.clone(),
            project_id: project_id.to_string(),
            sections,
            memory_map,
            citations,
            policies: {
                let mut policies = vec![
                    "query clean first, add strategy hints only after retrieval".to_string(),
                    "facts constrain answers but do not replace evidence".to_string(),
                ];
                if fact_focus {
                    policies.push(
                        "fact-focused query: present matched facts before supporting evidence"
                            .to_string(),
                    );
                }
                policies
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::agent::{AgentHabitat, AgentProfile, PersonaProfile, SkillProfile};
    use crate::facts::Fact;
    use crate::model::AgentRole;
    use crate::retrieval::SearchHit;

    use super::ContextPackBuilder;

    fn test_agent() -> AgentProfile {
        AgentProfile {
            id: "builder".to_string(),
            display_name: "Builder".to_string(),
            role: AgentRole::Builder,
            mission: "Test".to_string(),
            persona: PersonaProfile {
                voice: "direct".to_string(),
                initiative: 70,
                risk_appetite: 40,
                explanation_depth: 50,
            },
            habitat: AgentHabitat {
                home_space: "facts".to_string(),
                accessible_spaces: BTreeSet::new(),
                watch_spaces: BTreeSet::new(),
            },
            skill_profile: SkillProfile {
                domains: BTreeMap::new(),
                preferred_capabilities: BTreeSet::new(),
            },
            memory_priorities: BTreeMap::new(),
            manual_capability_modes: BTreeMap::new(),
        }
    }

    #[test]
    fn fact_focus_puts_fact_matches_first() {
        let builder = ContextPackBuilder::default();
        let fact = Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 85,
            evidence_ids: vec!["decision-1".to_string()],
        };
        let hit = SearchHit {
            chunk_id: "chunk-1".to_string(),
            space_id: "architecture".to_string(),
            space_path: vec!["Workspace Root".to_string(), "Architecture".to_string()],
            source_path: "src/mcp.rs".to_string(),
            line_start: 1,
            line_end: 3,
            ordinal: 0,
            score: 91,
            memory_path_match_count: 1,
            snippet: "colmem mcp runtime".to_string(),
            evidence_ids: vec!["chunk-1".to_string()],
            reasons: vec!["fact alignment: colmem supports mcp".to_string()],
        };

        let pack = builder.build(&test_agent(), "colmem", &[hit], &[fact], true);

        assert_eq!(
            pack.sections.first().map(|section| section.title.as_str()),
            Some("Fact Matches")
        );
        assert!(
            pack.sections
                .iter()
                .any(|section| section.title == "Fact Evidence")
        );
        assert!(
            pack.policies
                .iter()
                .any(|policy| policy.contains("fact-focused query"))
        );
        assert_eq!(pack.memory_map.len(), 1);
        assert_eq!(pack.memory_map[0].space_id, "architecture");
        assert_eq!(
            pack.memory_map[0].memory_path,
            "Workspace Root > Architecture"
        );
        assert!(pack.to_json().contains("\"memory_map\""));
    }
}
