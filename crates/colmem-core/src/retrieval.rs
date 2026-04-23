use std::collections::{BTreeMap, BTreeSet};

use crate::agent::AgentProfile;
use crate::host::HostContext;
use crate::model::TaskKind;
use crate::project::ProjectScope;
use crate::record::IndexState;
use crate::rerank::{LightweightReranker, RerankCandidate, RerankFactHint, SourceKind};
use crate::space::SpaceGraph;
use crate::utils::{is_meaningful_token, json_array, json_object, quote};

#[derive(Clone, Debug)]
pub enum RetrievalMode {
    Hybrid,
    StructureFirst,
    VectorFirst,
}

impl RetrievalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::StructureFirst => "structure_first",
            Self::VectorFirst => "vector_first",
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub text: String,
    pub project_id: String,
    pub task_kind: TaskKind,
    pub seed_space: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RetrievalPlan {
    pub mode: RetrievalMode,
    pub candidate_spaces: BTreeSet<String>,
    pub enable_full_text: bool,
    pub enable_vectors: bool,
    pub enable_facts: bool,
    pub enable_rerank: bool,
    pub notes: Vec<String>,
}

impl RetrievalPlan {
    pub fn to_json(&self) -> String {
        json_object([
            ("mode".to_string(), quote(self.mode.as_str())),
            (
                "candidate_spaces".to_string(),
                json_array(self.candidate_spaces.iter().map(|space| quote(space))),
            ),
            (
                "enable_full_text".to_string(),
                self.enable_full_text.to_string(),
            ),
            (
                "enable_vectors".to_string(),
                self.enable_vectors.to_string(),
            ),
            ("enable_facts".to_string(), self.enable_facts.to_string()),
            ("enable_rerank".to_string(), self.enable_rerank.to_string()),
            (
                "notes".to_string(),
                json_array(self.notes.iter().map(|note| quote(note))),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub chunk_id: String,
    pub space_id: String,
    pub space_path: Vec<String>,
    pub source_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub ordinal: usize,
    pub score: u8,
    pub memory_path_match_count: usize,
    pub snippet: String,
    pub evidence_ids: Vec<String>,
    pub reasons: Vec<String>,
}

impl SearchHit {
    pub fn memory_path(&self) -> String {
        if self.space_path.is_empty() {
            self.space_id.clone()
        } else {
            self.space_path.join(" > ")
        }
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("chunk_id".to_string(), quote(&self.chunk_id)),
            ("space_id".to_string(), quote(&self.space_id)),
            (
                "space_path".to_string(),
                json_array(self.space_path.iter().map(|space| quote(space))),
            ),
            ("memory_path".to_string(), quote(&self.memory_path())),
            ("source_path".to_string(), quote(&self.source_path)),
            ("line_start".to_string(), self.line_start.to_string()),
            ("line_end".to_string(), self.line_end.to_string()),
            ("ordinal".to_string(), self.ordinal.to_string()),
            ("score".to_string(), self.score.to_string()),
            (
                "memory_path_match_count".to_string(),
                self.memory_path_match_count.to_string(),
            ),
            ("snippet".to_string(), quote(&self.snippet)),
            (
                "evidence_ids".to_string(),
                json_array(self.evidence_ids.iter().map(|id| quote(id))),
            ),
            (
                "reasons".to_string(),
                json_array(self.reasons.iter().map(|reason| quote(reason))),
            ),
        ])
    }
}

#[derive(Clone, Debug)]
pub struct HybridRetriever {
    pub default_mode: RetrievalMode,
    pub reranker: LightweightReranker,
}

impl Default for HybridRetriever {
    fn default() -> Self {
        Self {
            default_mode: RetrievalMode::Hybrid,
            reranker: LightweightReranker::default(),
        }
    }
}

impl HybridRetriever {
    fn tokenize(text: &str) -> Vec<String> {
        text.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .filter(|token| is_meaningful_token(token))
            .map(|token| token.to_string())
            .collect()
    }

    fn preferred_space(
        chunk: &crate::record::Chunk,
        candidate_spaces: &BTreeSet<String>,
        query_tokens: &[String],
    ) -> String {
        let mut spaces = chunk
            .space_ids
            .iter()
            .filter(|space| candidate_spaces.contains(*space))
            .cloned()
            .collect::<Vec<_>>();
        if spaces.is_empty() {
            spaces = chunk.space_ids.iter().cloned().collect();
        }

        spaces
            .into_iter()
            .max_by(|left, right| {
                let left_score = query_tokens
                    .iter()
                    .filter(|token| left.contains(token.as_str()) || token.contains(left.as_str()))
                    .count();
                let right_score = query_tokens
                    .iter()
                    .filter(|token| {
                        right.contains(token.as_str()) || token.contains(right.as_str())
                    })
                    .count();
                left_score
                    .cmp(&right_score)
                    .then_with(|| right.len().cmp(&left.len()))
                    .then_with(|| right.cmp(left))
            })
            .unwrap_or_else(|| "workspace_root".to_string())
    }

    fn hash_index(value: &str, dimensions: usize) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        value.hash(&mut hasher);
        (hasher.finish() as usize) % dimensions.max(1)
    }

    fn char_ngrams(text: &str) -> Vec<String> {
        let compact = text
            .to_ascii_lowercase()
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<Vec<_>>();
        if compact.len() < 3 {
            return Vec::new();
        }

        compact
            .windows(3)
            .map(|window| window.iter().collect::<String>())
            .collect()
    }

    fn vectorize_text(text: &str, dimensions: usize) -> Vec<f32> {
        let mut values = vec![0.0f32; dimensions];
        for token in Self::tokenize(text) {
            let idx = Self::hash_index(&token, dimensions);
            values[idx] += 1.0;
        }
        for trigram in Self::char_ngrams(text) {
            let idx = Self::hash_index(&format!("tri:{trigram}"), dimensions);
            values[idx] += 0.35;
        }
        let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut values {
                *value /= norm;
            }
        }
        values
    }

    pub fn signature_vector(text: &str, dimensions: usize) -> Vec<f32> {
        Self::vectorize_text(text, dimensions)
    }

    fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
        left.iter()
            .zip(right.iter())
            .map(|(left_value, right_value)| left_value * right_value)
            .sum()
    }

    fn classify_source_kind(chunk: &crate::record::Chunk) -> SourceKind {
        match chunk.source_kind {
            crate::record::ChunkSourceKind::Implementation => SourceKind::Implementation,
            crate::record::ChunkSourceKind::Test => SourceKind::Test,
            crate::record::ChunkSourceKind::Documentation => SourceKind::Documentation,
            crate::record::ChunkSourceKind::Config => SourceKind::Config,
            crate::record::ChunkSourceKind::Plan => SourceKind::Plan,
            crate::record::ChunkSourceKind::Generated => SourceKind::Generated,
        }
    }

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

    fn source_tiebreak_priority(
        query_tokens: &[String],
        fact_hints_present: bool,
        hit: &SearchHit,
    ) -> i32 {
        let path = hit.source_path.to_ascii_lowercase();
        let is_doc = path.starts_with("docs/") || path.ends_with(".md") || path.ends_with(".txt");
        let is_test = path.contains("/tests/") || path.contains("\\tests\\");
        let is_source = path.starts_with("src/");

        if Self::query_prefers_tests(query_tokens) {
            return if is_test {
                3
            } else if is_source {
                2
            } else {
                1
            };
        }
        if Self::query_prefers_docs(query_tokens) {
            return if is_doc {
                3
            } else if is_source {
                2
            } else {
                1
            };
        }
        if fact_hints_present {
            return if is_source {
                3
            } else if is_doc {
                1
            } else {
                0
            };
        }
        if is_source {
            3
        } else if is_doc {
            1
        } else if is_test {
            0
        } else {
            2
        }
    }

    pub fn plan(
        &self,
        graph: &SpaceGraph,
        project: &ProjectScope,
        agent: &AgentProfile,
        host: &HostContext,
        request: &QueryRequest,
    ) -> RetrievalPlan {
        let mut candidate_spaces = graph.candidate_spaces(&agent.habitat, project, 1);
        if let Some(seed_space) = &request.seed_space {
            candidate_spaces.insert(seed_space.clone());
        }

        let mut notes = vec![
            format!("host={}", host.host_id().as_str()),
            format!("project={}", project.id),
            format!("task={}", request.task_kind.as_str()),
            "strategy=space_tree+space_links+full_text+vectors+facts".to_string(),
        ];
        if !agent.habitat.watch_spaces.is_empty() {
            notes.push(format!(
                "agent_watch_spaces={}",
                agent
                    .habitat
                    .watch_spaces
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }

        RetrievalPlan {
            mode: self.default_mode.clone(),
            candidate_spaces,
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes,
        }
    }

    pub fn demo_hits(&self, request: &QueryRequest, plan: &RetrievalPlan) -> Vec<SearchHit> {
        plan.candidate_spaces
            .iter()
            .take(3)
            .enumerate()
            .map(|(index, space_id)| SearchHit {
                chunk_id: format!("chunk-{}-{index}", request.project_id),
                space_id: space_id.clone(),
                space_path: vec![space_id.clone()],
                source_path: "demo://generated".to_string(),
                line_start: 1,
                line_end: 1,
                ordinal: index,
                score: (88i16 - (index as i16 * 7)).max(1) as u8,
                memory_path_match_count: 0,
                snippet: format!(
                    "Context anchored in space '{space_id}' for query '{}'.",
                    request.text
                ),
                evidence_ids: vec![format!("evidence-{space_id}-{index}")],
                reasons: vec![
                    "matched habitat candidate space".to_string(),
                    "eligible for hybrid retrieval".to_string(),
                ],
            })
            .collect()
    }

    pub fn index_hits(
        &self,
        index: &IndexState,
        request: &QueryRequest,
        plan: &RetrievalPlan,
        fact_hints: &[RerankFactHint],
        max_results: usize,
    ) -> Vec<SearchHit> {
        let query_tokens = Self::tokenize(&request.text);
        let query_lower = request.text.to_ascii_lowercase();
        let query_vector = if index.vector.dimensions > 0 {
            Self::vectorize_text(&request.text, index.vector.dimensions)
        } else {
            Vec::new()
        };

        let chunk_lookup = index
            .chunks
            .iter()
            .filter(|chunk| chunk.project_id == request.project_id)
            .map(|chunk| (chunk.id.clone(), chunk))
            .collect::<BTreeMap<_, _>>();
        let vector_lookup = index
            .vector
            .chunks
            .iter()
            .filter_map(|entry| {
                if chunk_lookup.contains_key(&entry.chunk_id) {
                    Some((entry.chunk_id.clone(), &entry.values))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();
        let mut score_map = BTreeMap::<String, i32>::new();
        let mut matched_by_chunk = BTreeMap::<String, BTreeSet<String>>::new();
        let mut posting_hits_by_chunk = BTreeMap::<String, usize>::new();
        let mut vector_similarity_by_chunk = BTreeMap::<String, f32>::new();

        if plan.enable_full_text {
            for token in &query_tokens {
                if let Some(postings) = index.full_text.postings.get(token) {
                    for posting in postings {
                        if !chunk_lookup.contains_key(&posting.chunk_id) {
                            continue;
                        }
                        *score_map.entry(posting.chunk_id.clone()).or_insert(0) +=
                            8 + i32::from(posting.frequency.min(5)) * 3;
                        matched_by_chunk
                            .entry(posting.chunk_id.clone())
                            .or_default()
                            .insert(token.clone());
                        *posting_hits_by_chunk
                            .entry(posting.chunk_id.clone())
                            .or_insert(0) += 1;
                    }
                }
            }
        }

        if plan.enable_vectors && !query_vector.is_empty() {
            for (chunk_id, values) in &vector_lookup {
                let similarity = Self::cosine_similarity(&query_vector, values);
                if similarity < 0.18 {
                    continue;
                }
                vector_similarity_by_chunk.insert(chunk_id.clone(), similarity);
                *score_map.entry(chunk_id.clone()).or_insert(0) += (similarity * 18.0) as i32;
            }
        }

        let candidates = chunk_lookup
            .values()
            .filter_map(|chunk| {
                let text = chunk.text.to_ascii_lowercase();
                let path = chunk.source_path.to_ascii_lowercase();
                let matched_tokens = matched_by_chunk
                    .get(&chunk.id)
                    .map(|tokens| tokens.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                let vector_similarity = vector_similarity_by_chunk
                    .get(&chunk.id)
                    .copied()
                    .unwrap_or_default();
                if matched_tokens.is_empty() && vector_similarity <= 0.0 {
                    return None;
                }

                let exact_phrase_bonus = if text.contains(&query_lower) { 18 } else { 0 };
                let exact_phrase_bonus = if matched_tokens.is_empty() {
                    exact_phrase_bonus / 2
                } else {
                    exact_phrase_bonus
                };
                let path_bonus = query_tokens
                    .iter()
                    .filter(|token| path.contains(token.as_str()))
                    .count() as i32
                    * 5;
                let space_bonus = if chunk
                    .space_ids
                    .iter()
                    .any(|space| plan.candidate_spaces.contains(space))
                {
                    12
                } else {
                    0
                };
                let posting_bonus = posting_hits_by_chunk
                    .get(&chunk.id)
                    .copied()
                    .unwrap_or_default() as i32
                    * 2;
                let total_score = score_map.get(&chunk.id).copied().unwrap_or_default()
                    + exact_phrase_bonus
                    + path_bonus
                    + space_bonus
                    + posting_bonus;
                let preferred_space =
                    Self::preferred_space(chunk, &plan.candidate_spaces, &query_tokens);
                let memory_path = chunk
                    .space_paths
                    .get(&preferred_space)
                    .cloned()
                    .unwrap_or_else(|| vec![preferred_space.clone()]);
                let memory_path_text = memory_path.join(" ").to_ascii_lowercase();
                let memory_path_match_count = query_tokens
                    .iter()
                    .filter(|token| memory_path_text.contains(token.as_str()))
                    .count();

                let mut initial_reasons = Vec::new();
                if !matched_tokens.is_empty() {
                    initial_reasons.push(format!(
                        "matched terms: {}",
                        matched_tokens
                            .iter()
                            .take(4)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    initial_reasons.push("scored from full-text postings".to_string());
                }

                Some((
                    RerankCandidate {
                        chunk_id: chunk.id.clone(),
                        source_path: chunk.source_path.clone(),
                        search_text: chunk.text.to_ascii_lowercase(),
                        base_score: total_score,
                        source_kind: Self::classify_source_kind(chunk),
                        matched_tokens: matched_tokens.clone(),
                        vector_similarity,
                        exact_phrase: exact_phrase_bonus > 0,
                        path_match_count: query_tokens
                            .iter()
                            .filter(|token| path.contains(token.as_str()))
                            .count(),
                        candidate_space_match: space_bonus > 0,
                        initial_reasons,
                    },
                    SearchHit {
                        chunk_id: chunk.id.clone(),
                        space_id: preferred_space.clone(),
                        space_path: memory_path,
                        source_path: chunk.source_path.clone(),
                        line_start: chunk.line_start,
                        line_end: chunk.line_end,
                        ordinal: chunk.ordinal,
                        score: 0,
                        memory_path_match_count,
                        snippet: chunk.text.chars().take(220).collect(),
                        evidence_ids: vec![chunk.id.clone(), chunk.record_id.clone()],
                        reasons: Vec::new(),
                    },
                ))
            })
            .collect::<Vec<_>>();
        let mut hits = if plan.enable_rerank {
            let empty_fact_hints = Vec::new();
            let effective_fact_hints = if plan.enable_facts {
                fact_hints
            } else {
                &empty_fact_hints
            };
            let reranked = self.reranker.rerank(
                &request.task_kind,
                &query_tokens,
                effective_fact_hints,
                candidates
                    .iter()
                    .map(|(candidate, _)| candidate.clone())
                    .collect(),
            );
            let rerank_lookup = reranked
                .into_iter()
                .map(|result| (result.chunk_id.clone(), result))
                .collect::<BTreeMap<_, _>>();

            candidates
                .into_iter()
                .filter_map(|(_, mut hit)| {
                    let result = rerank_lookup.get(&hit.chunk_id)?;
                    hit.score = result.final_score;
                    hit.reasons = result.reasons.clone();
                    Some(hit)
                })
                .collect::<Vec<_>>()
        } else {
            candidates
                .into_iter()
                .map(|(candidate, mut hit)| {
                    hit.score = candidate.base_score.clamp(1, 99) as u8;
                    hit.reasons = candidate.initial_reasons;
                    hit
                })
                .collect::<Vec<_>>()
        };
        let facts_enabled_for_tiebreak = plan.enable_facts && !fact_hints.is_empty();
        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| {
                    Self::source_tiebreak_priority(&query_tokens, facts_enabled_for_tiebreak, right)
                        .cmp(&Self::source_tiebreak_priority(
                            &query_tokens,
                            facts_enabled_for_tiebreak,
                            left,
                        ))
                })
                .then_with(|| left.source_path.cmp(&right.source_path))
                .then_with(|| left.ordinal.cmp(&right.ordinal))
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(max_results);
        hits
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::ingest::build_project_index;
    use crate::model::TaskKind;
    use crate::project::ProjectScope;
    use crate::record::{Chunk, FullTextIndex, IndexState, TokenPosting, VectorChunk, VectorIndex};
    use crate::rerank::{RerankFactHint, SourceKind};

    use super::{HybridRetriever, QueryRequest, RetrievalMode, RetrievalPlan};

    fn temp_dir() -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("colmem-retrieval-golden-{stamp}"));
        root
    }

    #[test]
    fn index_hits_prefers_path_and_candidate_space_matches() {
        let retriever = HybridRetriever::default();
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::from([
                    (
                        "retrieval".to_string(),
                        vec![TokenPosting {
                            chunk_id: "chunk-a".to_string(),
                            frequency: 2,
                        }],
                    ),
                    (
                        "ranking".to_string(),
                        vec![TokenPosting {
                            chunk_id: "chunk-a".to_string(),
                            frequency: 1,
                        }],
                    ),
                ]),
            },
            vector: VectorIndex {
                version: 1,
                dimensions: 8,
                chunks: vec![
                    VectorChunk {
                        chunk_id: "chunk-a".to_string(),
                        values: vec![0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    },
                    VectorChunk {
                        chunk_id: "chunk-b".to_string(),
                        values: vec![0.0, 0.0, 0.8, 0.2, 0.0, 0.0, 0.0, 0.0],
                    },
                ],
            },
            records: Vec::new(),
            chunks: vec![
                Chunk {
                    id: "chunk-a".to_string(),
                    record_id: "record-a".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/retrieval.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 10,
                    line_end: 20,
                    char_count: 80,
                    text: "Hybrid retrieval merges lexical search with space-aware ranking."
                        .to_string(),
                    space_ids: BTreeSet::from(["retrieval".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "a".to_string(),
                },
                Chunk {
                    id: "chunk-b".to_string(),
                    record_id: "record-b".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 6,
                    char_count: 60,
                    text: "Agent personas can evolve over time.".to_string(),
                    space_ids: BTreeSet::from(["agent_runtime".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "b".to_string(),
                },
            ],
        };
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from(["retrieval".to_string()]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };
        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "retrieval ranking".to_string(),
                project_id: "colmem".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            5,
        );

        assert_eq!(
            hits.first().map(|hit| hit.chunk_id.as_str()),
            Some("chunk-a")
        );
        assert_eq!(
            hits.first().map(|hit| hit.source_path.as_str()),
            Some("src/retrieval.rs")
        );
        assert!(
            hits.first()
                .map(|hit| hit.memory_path_match_count > 0)
                .unwrap_or(false)
        );
    }

    #[test]
    fn index_hits_honors_disabled_facts_in_final_tiebreak() {
        let retriever = HybridRetriever::default();
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::from([(
                    "memory".to_string(),
                    vec![
                        TokenPosting {
                            chunk_id: "doc".to_string(),
                            frequency: 1,
                        },
                        TokenPosting {
                            chunk_id: "other".to_string(),
                            frequency: 1,
                        },
                    ],
                )]),
            },
            vector: VectorIndex {
                version: 1,
                dimensions: 0,
                chunks: Vec::new(),
            },
            records: Vec::new(),
            chunks: vec![
                Chunk {
                    id: "doc".to_string(),
                    record_id: "record-doc".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "docs/memory.txt".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Documentation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 1,
                    char_count: 20,
                    text: "memory calibration".to_string(),
                    space_ids: BTreeSet::from(["retrieval".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "doc".to_string(),
                },
                Chunk {
                    id: "other".to_string(),
                    record_id: "record-other".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "notes/memory.log".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Documentation,
                    ordinal: 1,
                    line_start: 1,
                    line_end: 1,
                    char_count: 20,
                    text: "memory calibration".to_string(),
                    space_ids: BTreeSet::from(["retrieval".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "other".to_string(),
                },
            ],
        };
        let request = QueryRequest {
            text: "memory".to_string(),
            project_id: "colmem".to_string(),
            task_kind: TaskKind::Query,
            seed_space: Some("retrieval".to_string()),
        };
        let fact_hints = vec![RerankFactHint {
            summary: "memory fact".to_string(),
            tokens: vec!["memory".to_string()],
            confidence: 90,
            reason: None,
        }];
        let mut plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from(["retrieval".to_string()]),
            enable_full_text: true,
            enable_vectors: false,
            enable_facts: false,
            enable_rerank: false,
            notes: Vec::new(),
        };
        let without_facts = retriever.index_hits(&index, &request, &plan, &fact_hints, 2);
        assert_eq!(without_facts[0].chunk_id, "other");

        plan.enable_facts = true;
        let with_facts = retriever.index_hits(&index, &request, &plan, &fact_hints, 2);
        assert_eq!(with_facts[0].chunk_id, "doc");
    }

    #[test]
    fn index_hits_can_return_vector_only_match() {
        let retriever = HybridRetriever::default();
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::new(),
            },
            vector: VectorIndex {
                version: 1,
                dimensions: 64,
                chunks: vec![VectorChunk {
                    chunk_id: "chunk-evolve".to_string(),
                    values: HybridRetriever::vectorize_text("evolve persona agent memory", 64),
                }],
            },
            records: Vec::new(),
            chunks: vec![Chunk {
                id: "chunk-evolve".to_string(),
                record_id: "record-evolve".to_string(),
                project_id: "colmem".to_string(),
                source_path: "src/agent.rs".to_string(),
                source_kind: crate::record::ChunkSourceKind::Implementation,
                ordinal: 0,
                line_start: 1,
                line_end: 4,
                char_count: 40,
                text: "Agents evolve their persona over time.".to_string(),
                space_ids: BTreeSet::from(["agent_runtime".to_string()]),
                space_paths: BTreeMap::new(),
                hash: "evolve".to_string(),
            }],
        };
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from(["agent_runtime".to_string()]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };
        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "evolutionary persona".to_string(),
                project_id: "colmem".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            5,
        );

        assert_eq!(hits.len(), 1);
        assert!(
            hits[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("vector similarity"))
        );
    }

    #[test]
    fn implementation_code_beats_test_code_for_generic_query() {
        let retriever = HybridRetriever::default();
        let code_vector = HybridRetriever::vectorize_text("persona evolve agent profile", 64);
        let test_vector = HybridRetriever::vectorize_text("persona evolve agent profile", 64);
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::from([
                    (
                        "persona".to_string(),
                        vec![
                            TokenPosting {
                                chunk_id: "chunk-code".to_string(),
                                frequency: 1,
                            },
                            TokenPosting {
                                chunk_id: "chunk-test".to_string(),
                                frequency: 1,
                            },
                        ],
                    ),
                    (
                        "evolve".to_string(),
                        vec![
                            TokenPosting {
                                chunk_id: "chunk-code".to_string(),
                                frequency: 1,
                            },
                            TokenPosting {
                                chunk_id: "chunk-test".to_string(),
                                frequency: 1,
                            },
                        ],
                    ),
                ]),
            },
            vector: VectorIndex {
                version: 1,
                dimensions: 64,
                chunks: vec![
                    VectorChunk {
                        chunk_id: "chunk-code".to_string(),
                        values: code_vector,
                    },
                    VectorChunk {
                        chunk_id: "chunk-test".to_string(),
                        values: test_vector,
                    },
                ],
            },
            records: Vec::new(),
            chunks: vec![
                Chunk {
                    id: "chunk-code".to_string(),
                    record_id: "record-code".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 4,
                    char_count: 40,
                    text: "Agent persona can evolve through profile patches.".to_string(),
                    space_ids: BTreeSet::from(["agent_runtime".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "code".to_string(),
                },
                Chunk {
                    id: "chunk-test".to_string(),
                    record_id: "record-test".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/retrieval.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Test,
                    ordinal: 1,
                    line_start: 500,
                    line_end: 520,
                    char_count: 50,
                    text: "#[test]\nfn persona_evolve_test() { assert!(true); }".to_string(),
                    space_ids: BTreeSet::from(["retrieval".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "test".to_string(),
                },
            ],
        };
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from([
                "agent_runtime".to_string(),
                "retrieval".to_string(),
            ]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };
        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "persona evolve".to_string(),
                project_id: "colmem".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            5,
        );

        assert_eq!(
            hits.first().map(|hit| hit.chunk_id.as_str()),
            Some("chunk-code")
        );
        assert!(
            hits[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("prefer implementation code"))
        );
    }

    #[test]
    fn fact_hints_boost_aligned_chunk() {
        let retriever = HybridRetriever::default();
        let index = IndexState {
            version: 1,
            full_text: FullTextIndex {
                version: 1,
                postings: BTreeMap::from([(
                    "colmem".to_string(),
                    vec![
                        TokenPosting {
                            chunk_id: "chunk-agent".to_string(),
                            frequency: 1,
                        },
                        TokenPosting {
                            chunk_id: "chunk-retrieval".to_string(),
                            frequency: 1,
                        },
                    ],
                )]),
            },
            vector: VectorIndex {
                version: 1,
                dimensions: 64,
                chunks: vec![
                    VectorChunk {
                        chunk_id: "chunk-agent".to_string(),
                        values: HybridRetriever::vectorize_text("colmem agent runtime", 64),
                    },
                    VectorChunk {
                        chunk_id: "chunk-retrieval".to_string(),
                        values: HybridRetriever::vectorize_text("colmem hybrid retrieval", 64),
                    },
                ],
            },
            records: Vec::new(),
            chunks: vec![
                Chunk {
                    id: "chunk-agent".to_string(),
                    record_id: "record-agent".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/agent.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 3,
                    char_count: 30,
                    text: "colmem agent runtime".to_string(),
                    space_ids: BTreeSet::from(["agent_runtime".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "agent".to_string(),
                },
                Chunk {
                    id: "chunk-retrieval".to_string(),
                    record_id: "record-retrieval".to_string(),
                    project_id: "colmem".to_string(),
                    source_path: "src/retrieval.rs".to_string(),
                    source_kind: crate::record::ChunkSourceKind::Implementation,
                    ordinal: 0,
                    line_start: 1,
                    line_end: 3,
                    char_count: 30,
                    text: "colmem hybrid retrieval".to_string(),
                    space_ids: BTreeSet::from(["retrieval".to_string()]),
                    space_paths: BTreeMap::new(),
                    hash: "retrieval".to_string(),
                },
            ],
        };
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from([
                "agent_runtime".to_string(),
                "retrieval".to_string(),
            ]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };
        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "colmem".to_string(),
                project_id: "colmem".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[RerankFactHint {
                summary: "colmem prefers hybrid retrieval".to_string(),
                tokens: vec![
                    "colmem".to_string(),
                    "hybrid".to_string(),
                    "retrieval".to_string(),
                ],
                confidence: 93,
                reason: Some("currently active, latest active fact".to_string()),
            }],
            5,
        );

        assert_eq!(
            hits.first().map(|hit| hit.chunk_id.as_str()),
            Some("chunk-retrieval")
        );
        assert!(
            hits[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("fact alignment"))
        );
    }

    #[test]
    fn test_like_assert_chunk_is_classified_as_test() {
        let chunk = Chunk {
            id: "chunk-test".to_string(),
            record_id: "record-test".to_string(),
            project_id: "colmem".to_string(),
            source_path: "src/facts.rs".to_string(),
            source_kind: crate::record::ChunkSourceKind::Test,
            ordinal: 0,
            line_start: 1,
            line_end: 3,
            char_count: 64,
            text: "assert_eq!(matched[0].predicate, \"supports\");".to_string(),
            space_ids: BTreeSet::from(["facts".to_string()]),
            space_paths: BTreeMap::new(),
            hash: "hash".to_string(),
        };

        assert_eq!(
            HybridRetriever::classify_source_kind(&chunk),
            SourceKind::Test
        );
    }

    #[test]
    fn golden_queries_cover_implementation_docs_and_tests() {
        let root = temp_dir();
        fs::create_dir_all(root.join("src")).expect("create src dir");
        fs::create_dir_all(root.join("tests")).expect("create tests dir");
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(
            root.join("src/retrieval.rs"),
            "pub fn hybrid_retrieval() { /* hybrid retrieval ranking pipeline */ }\n",
        )
        .expect("write implementation fixture");
        fs::write(
            root.join("tests/retrieval.rs"),
            "#[test]\nfn retrieval_regression_test() { assert_eq!(2, 1 + 1); }\n",
        )
        .expect("write test fixture");
        fs::write(
            root.join("docs/guide.md"),
            "# Architecture Guide\nThis architecture guide explains retrieval overview and design.\n",
        )
        .expect("write doc fixture");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, _) = build_project_index(&project).expect("build fixture index");
        let retriever = HybridRetriever::default();
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from(["retrieval".to_string(), "architecture".to_string()]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };

        let implementation_hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "hybrid retrieval ranking".to_string(),
                project_id: "demo".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            3,
        );
        assert_eq!(
            implementation_hits
                .first()
                .map(|hit| hit.source_path.as_str()),
            Some("src/retrieval.rs")
        );

        let documentation_hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "architecture guide".to_string(),
                project_id: "demo".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            3,
        );
        assert_eq!(
            documentation_hits
                .first()
                .map(|hit| hit.source_path.as_str()),
            Some("docs/guide.md")
        );

        let test_hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "retrieval regression test".to_string(),
                project_id: "demo".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            3,
        );
        assert_eq!(
            test_hits.first().map(|hit| hit.source_path.as_str()),
            Some("tests/retrieval.rs")
        );
    }

    #[test]
    fn golden_fact_heavy_query_prefers_implementation_evidence_over_echoes() {
        let root = temp_dir();
        fs::create_dir_all(root.join("src")).expect("create src dir");
        fs::create_dir_all(root.join("tests")).expect("create tests dir");
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(
            root.join("src/mcp.rs"),
            "pub fn mcp_runtime() { /* colmem supports mcp stdio clients */ }\n",
        )
        .expect("write implementation fixture");
        fs::write(
            root.join("tests/facts.rs"),
            "#[test]\nfn supports_mcp_fact() { assert_eq!(\"colmem supports mcp\", \"colmem supports mcp\"); }\n",
        )
        .expect("write test fixture");
        fs::write(
            root.join("docs/facts.md"),
            "# Fact Notes\nThe phrase colmem supports mcp appears here as documentation echo.\n",
        )
        .expect("write doc fixture");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, _) = build_project_index(&project).expect("build fixture index");
        let retriever = HybridRetriever::default();
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from(["agent_runtime".to_string(), "facts".to_string()]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };

        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "colmem supports mcp".to_string(),
                project_id: "demo".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[RerankFactHint {
                summary: "colmem supports mcp".to_string(),
                tokens: vec![
                    "colmem".to_string(),
                    "supports".to_string(),
                    "mcp".to_string(),
                ],
                confidence: 90,
                reason: Some("golden fact".to_string()),
            }],
            3,
        );

        assert_eq!(
            hits.first().map(|hit| hit.source_path.as_str()),
            Some("src/mcp.rs"),
            "fact-heavy hits: {hits:#?}"
        );
        assert!(
            hits[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("fact alignment"))
        );
        assert_ne!(
            hits.first().map(|hit| hit.source_path.as_str()),
            Some("tests/facts.rs")
        );
    }

    #[test]
    fn golden_implementation_heavy_query_prefers_runtime_code() {
        let root = temp_dir();
        fs::create_dir_all(root.join("src")).expect("create src dir");
        fs::create_dir_all(root.join("tests")).expect("create tests dir");
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(
            root.join("src/capability.rs"),
            "pub fn enforce_capability_permissions() { /* capability permission enforcement write stdio stateful */ }\n",
        )
        .expect("write implementation fixture");
        fs::write(
            root.join("tests/harness.rs"),
            "#[test]\nfn capability_permission_enforcement_regression() { assert!(true); }\n",
        )
        .expect("write test fixture");
        fs::write(
            root.join("docs/capability.md"),
            "# Capability Permission Enforcement\nThis document describes capability permission enforcement.\n",
        )
        .expect("write doc fixture");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, _) = build_project_index(&project).expect("build fixture index");
        let retriever = HybridRetriever::default();
        let plan = RetrievalPlan {
            mode: RetrievalMode::Hybrid,
            candidate_spaces: BTreeSet::from([
                "agent_runtime".to_string(),
                "architecture".to_string(),
            ]),
            enable_full_text: true,
            enable_vectors: true,
            enable_facts: true,
            enable_rerank: true,
            notes: Vec::new(),
        };

        let hits = retriever.index_hits(
            &index,
            &QueryRequest {
                text: "capability permission enforcement".to_string(),
                project_id: "demo".to_string(),
                task_kind: TaskKind::Query,
                seed_space: None,
            },
            &plan,
            &[],
            3,
        );

        assert_eq!(
            hits.first().map(|hit| hit.source_path.as_str()),
            Some("src/capability.rs"),
            "implementation-heavy hits: {hits:#?}"
        );
        assert!(
            hits[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("prefer implementation code"))
        );
    }
}
