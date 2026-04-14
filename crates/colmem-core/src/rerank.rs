use serde::{Deserialize, Serialize};

use crate::model::TaskKind;

#[derive(Clone, Debug)]
pub struct RerankModelCandidate {
    pub chunk_id: String,
    pub source_path: String,
    pub text: String,
    pub current_score: u8,
}

#[derive(Clone, Debug)]
pub struct RerankModelRequest {
    pub query: String,
    pub candidates: Vec<RerankModelCandidate>,
}

#[derive(Clone, Debug)]
pub struct RerankModelScore {
    pub chunk_id: String,
    pub score: f32,
    pub reason: Option<String>,
}

pub trait ExternalRerankModel {
    fn rerank(&self, request: &RerankModelRequest) -> Result<Vec<RerankModelScore>, String>;
}

#[derive(Clone, Debug)]
pub struct RerankFactHint {
    pub summary: String,
    pub tokens: Vec<String>,
    pub confidence: u8,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceKind {
    Implementation,
    Test,
    Documentation,
    Config,
    Plan,
    Generated,
}

#[derive(Clone, Debug)]
pub struct RerankCandidate {
    pub chunk_id: String,
    pub source_path: String,
    pub search_text: String,
    pub base_score: i32,
    pub source_kind: SourceKind,
    pub matched_tokens: Vec<String>,
    pub vector_similarity: f32,
    pub exact_phrase: bool,
    pub path_match_count: usize,
    pub candidate_space_match: bool,
    pub initial_reasons: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RerankResult {
    pub chunk_id: String,
    pub final_score: u8,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceWeightConfig {
    pub implementation_default: i32,
    pub implementation_review: i32,
    pub implementation_refactor: i32,
    pub implementation_diagnose: i32,
    pub test_preferred: i32,
    pub test_generic: i32,
    pub documentation_preferred: i32,
    pub documentation_generic: i32,
    pub config_preferred: i32,
    pub config_generic: i32,
    pub plan_preferred: i32,
    pub plan_generic: i32,
    pub generated_generic: i32,
}

#[derive(Clone, Debug)]
pub struct ModuleAffinityFamily {
    pub label: String,
    pub path_fragment: String,
    pub keywords: Vec<String>,
    pub path_and_token_score: i32,
    pub path_only_score: i32,
}

#[derive(Clone, Debug)]
pub struct FactQueryConfig {
    pub enabled: bool,
    pub overlap_threshold: i32,
    pub exact_phrase_test_penalty: i32,
    pub test_penalty: i32,
    pub documentation_penalty: i32,
    pub implementation_facts_bonus: i32,
    pub enable_test_score_cap: bool,
    pub test_score_cap: i32,
}

#[derive(Clone, Debug)]
pub struct PrimitiveScoreConfig {
    pub path_match_per_token: i32,
    pub path_match_max_tokens: usize,
    pub candidate_space_match_bonus: i32,
}

impl Default for PrimitiveScoreConfig {
    fn default() -> Self {
        Self {
            path_match_per_token: 4,
            path_match_max_tokens: 3,
            candidate_space_match_bonus: 6,
        }
    }
}

impl Default for FactQueryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            overlap_threshold: 2,
            exact_phrase_test_penalty: -34,
            test_penalty: -34,
            documentation_penalty: -8,
            implementation_facts_bonus: 8,
            enable_test_score_cap: true,
            test_score_cap: 72,
        }
    }
}

impl Default for SourceWeightConfig {
    fn default() -> Self {
        Self {
            implementation_default: 10,
            implementation_review: 14,
            implementation_refactor: 16,
            implementation_diagnose: 12,
            test_preferred: 4,
            test_generic: -18,
            documentation_preferred: 12,
            documentation_generic: -4,
            config_preferred: 8,
            config_generic: -5,
            plan_preferred: 4,
            plan_generic: -14,
            generated_generic: -8,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LightweightRerankPolicy {
    pub source_weights: SourceWeightConfig,
    pub module_affinity_families: Vec<ModuleAffinityFamily>,
    pub fact_query: FactQueryConfig,
    pub primitive_scores: PrimitiveScoreConfig,
}

impl Default for ModuleAffinityFamily {
    fn default() -> Self {
        Self {
            label: "generic".to_string(),
            path_fragment: String::new(),
            keywords: Vec::new(),
            path_and_token_score: 6,
            path_only_score: 3,
        }
    }
}

impl Default for LightweightRerankPolicy {
    fn default() -> Self {
        Self {
            source_weights: SourceWeightConfig::default(),
            module_affinity_families: vec![
                ModuleAffinityFamily {
                    label: "agent".to_string(),
                    path_fragment: "agent".to_string(),
                    keywords: vec![
                        "agent",
                        "persona",
                        "evolution",
                        "patch",
                        "habitat",
                        "profile",
                        "skill",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "retrieval".to_string(),
                    path_fragment: "retrieval".to_string(),
                    keywords: vec![
                        "retrieval",
                        "search",
                        "index",
                        "vector",
                        "full",
                        "text",
                        "rerank",
                        "chunk",
                    ]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "facts".to_string(),
                    path_fragment: "facts".to_string(),
                    keywords: vec!["fact", "facts", "entity", "constraint", "evidence"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "context".to_string(),
                    path_fragment: "context".to_string(),
                    keywords: vec!["context", "citation", "policy", "section"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "storage".to_string(),
                    path_fragment: "storage".to_string(),
                    keywords: vec!["storage", "state", "persist", "workspace", "save", "load"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "mcp".to_string(),
                    path_fragment: "mcp".to_string(),
                    keywords: vec!["mcp", "tool", "protocol", "stdio", "server"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "project".to_string(),
                    path_fragment: "project".to_string(),
                    keywords: vec!["project", "scope", "attach", "root"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "capability".to_string(),
                    path_fragment: "capability".to_string(),
                    keywords: vec!["capability", "tool", "plugin", "skill", "registry"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
                ModuleAffinityFamily {
                    label: "host".to_string(),
                    path_fragment: "host".to_string(),
                    keywords: vec!["host", "cursor", "codex", "claude", "trae", "openclaw"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    path_and_token_score: 6,
                    path_only_score: 3,
                },
            ],
            fact_query: FactQueryConfig::default(),
            primitive_scores: PrimitiveScoreConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LightweightReranker {
    pub policy: LightweightRerankPolicy,
}

#[derive(Clone, Debug)]
struct SourceWeightPolicy {
    implementation: i32,
    test: i32,
    documentation: i32,
    config: i32,
    plan: i32,
    generated: i32,
}

impl LightweightReranker {
    fn query_prefers_docs(query_tokens: &[String]) -> bool {
        query_tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "readme"
                    | "docs"
                    | "document"
                    | "documentation"
                    | "roadmap"
                    | "plan"
                    | "architecture"
                    | "overview"
                    | "guide"
                    | "design"
                    | "spec"
            )
        })
    }

    fn query_prefers_tests(query_tokens: &[String]) -> bool {
        query_tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "test" | "tests" | "testing" | "assert" | "fixture" | "ci" | "regression"
            )
        })
    }

    fn query_prefers_config(query_tokens: &[String]) -> bool {
        query_tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "config" | "configuration" | "cargo" | "toml" | "json" | "yaml" | "env"
            )
        })
    }

    fn source_weight_policy(
        &self,
        query_tokens: &[String],
        task_kind: &TaskKind,
    ) -> SourceWeightPolicy {
        let prefers_docs = Self::query_prefers_docs(query_tokens);
        let prefers_tests = Self::query_prefers_tests(query_tokens);
        let prefers_config = Self::query_prefers_config(query_tokens);
        let weights = &self.policy.source_weights;

        let implementation = match task_kind {
            TaskKind::Review => weights.implementation_review,
            TaskKind::Refactor => weights.implementation_refactor,
            TaskKind::Diagnose => weights.implementation_diagnose,
            _ => weights.implementation_default,
        };
        let test = if prefers_tests || matches!(task_kind, TaskKind::Review | TaskKind::Diagnose) {
            weights.test_preferred
        } else {
            weights.test_generic
        };
        let documentation = if prefers_docs {
            weights.documentation_preferred
        } else {
            weights.documentation_generic
        };
        let config = if prefers_config || matches!(task_kind, TaskKind::Diagnose | TaskKind::Serve)
        {
            weights.config_preferred
        } else {
            weights.config_generic
        };
        let plan = if prefers_docs {
            weights.plan_preferred
        } else {
            weights.plan_generic
        };

        SourceWeightPolicy {
            implementation,
            test,
            documentation,
            config,
            plan,
            generated: weights.generated_generic,
        }
    }

    fn source_adjustment(
        &self,
        candidate: &RerankCandidate,
        query_tokens: &[String],
        task_kind: &TaskKind,
    ) -> (i32, String) {
        let policy = self.source_weight_policy(query_tokens, task_kind);

        match candidate.source_kind {
            SourceKind::Plan => {
                if policy.plan > 0 {
                    (4, "documentation plan match".to_string())
                } else {
                    (policy.plan, "deprioritized planning document".to_string())
                }
            }
            SourceKind::Test => {
                if policy.test > 0 {
                    (policy.test, "test evidence kept relevant".to_string())
                } else {
                    (policy.test, "deprioritized test code".to_string())
                }
            }
            SourceKind::Documentation => {
                if policy.documentation > 0 {
                    (policy.documentation, "documentation query".to_string())
                } else {
                    (
                        policy.documentation,
                        "slightly deprioritized documentation".to_string(),
                    )
                }
            }
            SourceKind::Config => {
                if policy.config > 0 {
                    (policy.config, "configuration query".to_string())
                } else {
                    (policy.config, "slightly deprioritized config".to_string())
                }
            }
            SourceKind::Generated => (
                policy.generated,
                "deprioritized generated output".to_string(),
            ),
            SourceKind::Implementation => (
                policy.implementation,
                match task_kind {
                    TaskKind::Review => "prefer implementation code for review".to_string(),
                    TaskKind::Refactor => "prefer implementation code for refactor".to_string(),
                    TaskKind::Diagnose => "prefer implementation code for diagnose".to_string(),
                    _ => "prefer implementation code".to_string(),
                },
            ),
        }
    }

    fn module_affinity(&self, path: &str, query_tokens: &[String]) -> (i32, Option<String>) {
        let path = path.to_ascii_lowercase();
        let mut best_label = None;
        let mut best_score = 0i32;

        for family in &self.policy.module_affinity_families {
            let path_match = path.contains(&family.path_fragment);
            let token_matches = query_tokens
                .iter()
                .filter(|token| family.keywords.iter().any(|keyword| keyword == *token))
                .count() as i32;
            let score = if path_match && token_matches > 0 {
                family.path_and_token_score + token_matches * 4
            } else if path_match {
                family.path_only_score
            } else {
                0
            };
            if score > best_score {
                best_score = score;
                best_label = Some(family.label.clone());
            }
        }

        if best_score > 0 {
            (
                best_score,
                best_label.map(|label| format!("module affinity matched '{label}'")),
            )
        } else {
            (0, None)
        }
    }

    fn path_match_adjustment(&self, count: usize) -> (i32, Option<String>) {
        if count == 0 {
            (0, None)
        } else {
            let policy = &self.policy.primitive_scores;
            (
                (count.min(policy.path_match_max_tokens) as i32) * policy.path_match_per_token,
                Some("query matched source path".to_string()),
            )
        }
    }

    fn candidate_space_adjustment(&self, matched: bool) -> (i32, Option<String>) {
        if matched {
            (
                self.policy.primitive_scores.candidate_space_match_bonus,
                Some("aligned with candidate space".to_string()),
            )
        } else {
            (0, None)
        }
    }

    fn fact_query_adjustment(
        &self,
        candidate: &RerankCandidate,
        query_tokens: &[String],
        fact_hints: &[RerankFactHint],
        best_fact_overlap: i32,
    ) -> (i32, Option<String>) {
        let policy = &self.policy.fact_query;
        if !policy.enabled || fact_hints.is_empty() || Self::query_prefers_tests(query_tokens) {
            return (0, None);
        }

        match candidate.source_kind {
            SourceKind::Test
                if best_fact_overlap >= policy.overlap_threshold || candidate.exact_phrase =>
            {
                (
                    if candidate.exact_phrase {
                        policy.exact_phrase_test_penalty
                    } else {
                        policy.test_penalty
                    },
                    Some("deprioritized test fixture for fact query".to_string()),
                )
            }
            SourceKind::Documentation if best_fact_overlap >= policy.overlap_threshold => (
                policy.documentation_penalty,
                Some("deprioritized documentation echo for fact query".to_string()),
            ),
            SourceKind::Implementation if best_fact_overlap >= policy.overlap_threshold => (
                policy.implementation_facts_bonus,
                Some("prefer fact-aligned implementation for fact query".to_string()),
            ),
            _ => (0, None),
        }
    }

    pub fn rerank(
        &self,
        task_kind: &TaskKind,
        query_tokens: &[String],
        fact_hints: &[RerankFactHint],
        mut candidates: Vec<RerankCandidate>,
    ) -> Vec<RerankResult> {
        let mut results = Vec::with_capacity(candidates.len());

        for candidate in candidates.drain(..) {
            let mut score = candidate.base_score;
            let mut reasons = candidate.initial_reasons.clone();

            let (source_delta, source_reason) =
                self.source_adjustment(&candidate, query_tokens, task_kind);
            score += source_delta;
            reasons.push(source_reason);

            let (module_delta, module_reason) =
                self.module_affinity(&candidate.source_path, query_tokens);
            score += module_delta;
            if let Some(reason) = module_reason {
                reasons.push(reason);
            }

            let (path_delta, path_reason) = self.path_match_adjustment(candidate.path_match_count);
            score += path_delta;
            if let Some(reason) = path_reason {
                reasons.push(reason);
            }

            let (space_delta, space_reason) =
                self.candidate_space_adjustment(candidate.candidate_space_match);
            score += space_delta;
            if let Some(reason) = space_reason {
                reasons.push(reason);
            }

            if candidate.exact_phrase {
                score += if candidate.matched_tokens.is_empty() {
                    9
                } else {
                    18
                };
                reasons.push("exact phrase matched chunk text".to_string());
            }

            if candidate.matched_tokens.len() >= 3 {
                score += 5;
            }

            if candidate.vector_similarity > 0.0 {
                reasons.push(format!(
                    "vector similarity={:.3}",
                    candidate.vector_similarity
                ));
                if candidate.matched_tokens.is_empty() {
                    score += (candidate.vector_similarity * 8.0) as i32;
                }
            }

            let search_blob = format!(
                "{} {}",
                candidate.source_path.to_ascii_lowercase(),
                candidate.search_text
            );
            let mut best_fact_reason = None;
            let mut best_fact_delta = 0;
            let mut best_fact_overlap = 0;
            for hint in fact_hints {
                let overlap = hint
                    .tokens
                    .iter()
                    .filter(|token| search_blob.contains(token.as_str()))
                    .count() as i32;
                if overlap == 0 {
                    continue;
                }
                let delta = overlap.min(3) * 3 + i32::from(hint.confidence.min(100)) / 20;
                if delta > best_fact_delta {
                    best_fact_delta = delta;
                    best_fact_overlap = overlap;
                    best_fact_reason = Some(match &hint.reason {
                        Some(reason) => format!("fact alignment: {} ({reason})", hint.summary),
                        None => format!("fact alignment: {}", hint.summary),
                    });
                }
            }
            if best_fact_delta > 0 {
                score += best_fact_delta;
                if let Some(reason) = best_fact_reason {
                    reasons.push(reason);
                }
            }

            let (fact_query_delta, fact_query_reason) =
                self.fact_query_adjustment(&candidate, query_tokens, fact_hints, best_fact_overlap);
            score += fact_query_delta;
            if let Some(reason) = fact_query_reason {
                reasons.push(reason);
            }
            if self.policy.fact_query.enabled
                && self.policy.fact_query.enable_test_score_cap
                && !fact_hints.is_empty()
                && !Self::query_prefers_tests(query_tokens)
                && candidate.source_kind == SourceKind::Test
                && (best_fact_overlap >= self.policy.fact_query.overlap_threshold
                    || candidate.exact_phrase)
            {
                score = score.min(self.policy.fact_query.test_score_cap);
                reasons.push("capped test fixture score for fact query".to_string());
            }

            results.push(RerankResult {
                chunk_id: candidate.chunk_id,
                final_score: score.clamp(1, 99) as u8,
                reasons,
            });
        }

        results.sort_by(|left, right| {
            right
                .final_score
                .cmp(&left.final_score)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        results
    }
}

#[cfg(test)]
mod tests {
    use crate::model::TaskKind;

    use super::{
        FactQueryConfig, LightweightRerankPolicy, LightweightReranker, ModuleAffinityFamily,
        PrimitiveScoreConfig, RerankCandidate, RerankFactHint, SourceKind, SourceWeightConfig,
    };

    #[test]
    fn implementation_candidate_beats_test_candidate() {
        let reranker = LightweightReranker::default();
        let query_tokens = vec!["persona".to_string(), "patch".to_string()];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[],
            vec![
                RerankCandidate {
                    chunk_id: "test".to_string(),
                    source_path: "src/retrieval.rs".to_string(),
                    search_text: "#[test] patch persona".to_string(),
                    base_score: 40,
                    source_kind: SourceKind::Test,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.5,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: persona, patch".to_string()],
                },
                RerankCandidate {
                    chunk_id: "impl".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    search_text: "impl patch persona".to_string(),
                    base_score: 38,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.5,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: persona, patch".to_string()],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("impl")
        );
    }

    #[test]
    fn fact_hint_boosts_matching_candidate() {
        let reranker = LightweightReranker::default();
        let query_tokens = vec!["memory".to_string()];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[RerankFactHint {
                summary: "colmem prefers hybrid retrieval".to_string(),
                tokens: vec![
                    "colmem".to_string(),
                    "hybrid".to_string(),
                    "retrieval".to_string(),
                ],
                confidence: 90,
                reason: Some("latest active fact".to_string()),
            }],
            vec![
                RerankCandidate {
                    chunk_id: "plain".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    search_text: "agent memory habitat".to_string(),
                    base_score: 40,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.3,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: memory".to_string()],
                },
                RerankCandidate {
                    chunk_id: "facty".to_string(),
                    source_path: "src/retrieval.rs".to_string(),
                    search_text: "colmem hybrid retrieval memory index".to_string(),
                    base_score: 38,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.3,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: memory".to_string()],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("facty")
        );
        assert!(
            results[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("fact alignment"))
        );
    }

    #[test]
    fn fact_query_deprioritizes_test_fixture() {
        let reranker = LightweightReranker::default();
        let query_tokens = vec![
            "colmem".to_string(),
            "supports".to_string(),
            "mcp".to_string(),
        ];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[RerankFactHint {
                summary: "colmem supports mcp".to_string(),
                tokens: vec![
                    "colmem".to_string(),
                    "supports".to_string(),
                    "mcp".to_string(),
                ],
                confidence: 85,
                reason: Some("currently active, latest active fact".to_string()),
            }],
            vec![
                RerankCandidate {
                    chunk_id: "test-fact".to_string(),
                    source_path: "src/facts.rs".to_string(),
                    search_text:
                        "#[test] assert_eq!(store.facts_for_query(\"colmem supports mcp\"), ...)"
                            .to_string(),
                    base_score: 60,
                    source_kind: SourceKind::Test,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.5,
                    exact_phrase: true,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: colmem, supports, mcp".to_string()],
                },
                RerankCandidate {
                    chunk_id: "impl-mcp".to_string(),
                    source_path: "src/mcp.rs".to_string(),
                    search_text: "colmem mcp runtime supports stdio clients".to_string(),
                    base_score: 54,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: vec!["colmem".to_string(), "mcp".to_string()],
                    vector_similarity: 0.4,
                    exact_phrase: false,
                    path_match_count: 1,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: colmem, mcp".to_string()],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("impl-mcp")
        );
        assert!(
            results
                .iter()
                .find(|result| result.chunk_id == "test-fact")
                .expect("test result")
                .reasons
                .iter()
                .any(|reason| reason.contains("deprioritized test fixture for fact query"))
        );
    }

    #[test]
    fn custom_source_weight_policy_can_prefer_documentation() {
        let reranker = LightweightReranker {
            policy: LightweightRerankPolicy {
                source_weights: SourceWeightConfig {
                    documentation_generic: 14,
                    implementation_default: 2,
                    ..SourceWeightConfig::default()
                },
                ..LightweightRerankPolicy::default()
            },
        };
        let query_tokens = vec!["overview".to_string()];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[],
            vec![
                RerankCandidate {
                    chunk_id: "impl".to_string(),
                    source_path: "src/runtime.rs".to_string(),
                    search_text: "runtime overview".to_string(),
                    base_score: 40,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.2,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: overview".to_string()],
                },
                RerankCandidate {
                    chunk_id: "docs".to_string(),
                    source_path: "docs/overview.md".to_string(),
                    search_text: "system overview".to_string(),
                    base_score: 36,
                    source_kind: SourceKind::Documentation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.2,
                    exact_phrase: false,
                    path_match_count: 1,
                    candidate_space_match: true,
                    initial_reasons: vec!["matched terms: overview".to_string()],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("docs")
        );
    }

    #[test]
    fn custom_module_affinity_policy_can_prefer_storage_module() {
        let reranker = LightweightReranker {
            policy: LightweightRerankPolicy {
                module_affinity_families: vec![ModuleAffinityFamily {
                    label: "storage".to_string(),
                    path_fragment: "storage".to_string(),
                    keywords: vec!["workspace".to_string(), "state".to_string()],
                    path_and_token_score: 20,
                    path_only_score: 1,
                }],
                ..LightweightRerankPolicy::default()
            },
        };
        let query_tokens = vec!["workspace".to_string(), "state".to_string()];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[],
            vec![
                RerankCandidate {
                    chunk_id: "agent".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    search_text: "workspace state migration".to_string(),
                    base_score: 50,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.4,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec![],
                },
                RerankCandidate {
                    chunk_id: "storage".to_string(),
                    source_path: "src/storage.rs".to_string(),
                    search_text: "workspace state migration".to_string(),
                    base_score: 46,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.4,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec![],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("storage")
        );
        assert!(
            results[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("module affinity matched 'storage'"))
        );
    }

    #[test]
    fn custom_fact_query_policy_can_disable_fact_specific_penalties() {
        let reranker = LightweightReranker {
            policy: LightweightRerankPolicy {
                fact_query: FactQueryConfig {
                    enabled: false,
                    ..FactQueryConfig::default()
                },
                ..LightweightRerankPolicy::default()
            },
        };
        let query_tokens = vec![
            "colmem".to_string(),
            "supports".to_string(),
            "mcp".to_string(),
        ];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[RerankFactHint {
                summary: "colmem supports mcp".to_string(),
                tokens: vec![
                    "colmem".to_string(),
                    "supports".to_string(),
                    "mcp".to_string(),
                ],
                confidence: 85,
                reason: Some("currently active, latest active fact".to_string()),
            }],
            vec![RerankCandidate {
                chunk_id: "test-fact".to_string(),
                source_path: "src/facts.rs".to_string(),
                search_text:
                    "#[test] assert_eq!(store.facts_for_query(\"colmem supports mcp\"), ...)"
                        .to_string(),
                base_score: 60,
                source_kind: SourceKind::Test,
                matched_tokens: query_tokens.clone(),
                vector_similarity: 0.5,
                exact_phrase: true,
                path_match_count: 0,
                candidate_space_match: true,
                initial_reasons: vec!["matched terms: colmem, supports, mcp".to_string()],
            }],
        );

        assert!(
            results[0]
                .reasons
                .iter()
                .all(|reason| !reason.contains("for fact query"))
        );
    }

    #[test]
    fn custom_primitive_score_policy_can_change_path_and_space_weights() {
        let reranker = LightweightReranker {
            policy: LightweightRerankPolicy {
                primitive_scores: PrimitiveScoreConfig {
                    path_match_per_token: 10,
                    path_match_max_tokens: 2,
                    candidate_space_match_bonus: 0,
                },
                ..LightweightRerankPolicy::default()
            },
        };
        let query_tokens = vec!["storage".to_string()];
        let results = reranker.rerank(
            &TaskKind::Query,
            &query_tokens,
            &[],
            vec![
                RerankCandidate {
                    chunk_id: "space".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    search_text: "storage".to_string(),
                    base_score: 46,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.1,
                    exact_phrase: false,
                    path_match_count: 0,
                    candidate_space_match: true,
                    initial_reasons: vec![],
                },
                RerankCandidate {
                    chunk_id: "path".to_string(),
                    source_path: "src/storage.rs".to_string(),
                    search_text: "storage".to_string(),
                    base_score: 42,
                    source_kind: SourceKind::Implementation,
                    matched_tokens: query_tokens.clone(),
                    vector_similarity: 0.1,
                    exact_phrase: false,
                    path_match_count: 1,
                    candidate_space_match: false,
                    initial_reasons: vec![],
                },
            ],
        );

        assert_eq!(
            results.first().map(|result| result.chunk_id.as_str()),
            Some("path")
        );
        assert!(
            results[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("query matched source path"))
        );
    }
}
