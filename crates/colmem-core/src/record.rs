use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::space::SpaceGraph;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RecordSourceType {
    ProjectFile,
    ConversationExport,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ChunkSourceKind {
    #[default]
    Implementation,
    Test,
    Documentation,
    Config,
    Plan,
    Generated,
}

impl ChunkSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implementation => "implementation",
            Self::Test => "test",
            Self::Documentation => "documentation",
            Self::Config => "config",
            Self::Plan => "plan",
            Self::Generated => "generated",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub project_id: String,
    pub source_type: RecordSourceType,
    pub source_path: String,
    #[serde(default)]
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub content_hash: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub record_id: String,
    pub project_id: String,
    pub source_path: String,
    #[serde(default)]
    pub source_kind: ChunkSourceKind,
    pub ordinal: usize,
    #[serde(default)]
    pub line_start: usize,
    #[serde(default)]
    pub line_end: usize,
    #[serde(default)]
    pub char_count: usize,
    pub text: String,
    pub space_ids: BTreeSet<String>,
    #[serde(default)]
    pub space_paths: BTreeMap<String, Vec<String>>,
    pub hash: String,
}

impl Chunk {
    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(&self.id)),
            ("record_id".to_string(), quote(&self.record_id)),
            ("project_id".to_string(), quote(&self.project_id)),
            ("source_path".to_string(), quote(&self.source_path)),
            ("source_kind".to_string(), quote(self.source_kind.as_str())),
            ("ordinal".to_string(), self.ordinal.to_string()),
            ("line_start".to_string(), self.line_start.to_string()),
            ("line_end".to_string(), self.line_end.to_string()),
            ("char_count".to_string(), self.char_count.to_string()),
            ("text".to_string(), quote(&self.text)),
            (
                "space_ids".to_string(),
                json_array(self.space_ids.iter().map(|space| quote(space))),
            ),
            (
                "space_paths".to_string(),
                json_object(self.space_paths.iter().map(|(space, path)| {
                    (
                        space.clone(),
                        json_array(path.iter().map(|segment| quote(segment))),
                    )
                })),
            ),
            ("hash".to_string(), quote(&self.hash)),
        ])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenPosting {
    pub chunk_id: String,
    pub frequency: u16,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FullTextIndex {
    pub version: u32,
    #[serde(default)]
    pub postings: BTreeMap<String, Vec<TokenPosting>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VectorChunk {
    pub chunk_id: String,
    #[serde(default)]
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VectorIndex {
    pub version: u32,
    pub dimensions: usize,
    #[serde(default)]
    pub chunks: Vec<VectorChunk>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexState {
    pub version: u32,
    #[serde(default)]
    pub full_text: FullTextIndex,
    #[serde(default)]
    pub vector: VectorIndex,
    pub records: Vec<Record>,
    pub chunks: Vec<Chunk>,
}

impl IndexState {
    pub fn infer_chunk_source_kind(source_path: &str, text: &str) -> ChunkSourceKind {
        let path = source_path.to_ascii_lowercase();
        let content = text.to_ascii_lowercase();
        let ext = path.rsplit('.').next().unwrap_or_default();
        let looks_like_test_body = content.contains("#[test]")
            || content.contains("assert_eq!(")
            || content.contains("assert!(")
            || content.contains("assert_ne!(")
            || content.contains("expect(\"")
            || content.contains("should_panic")
            || content.contains("mod tests")
            || (content.contains("fn ") && content.contains("test"));

        if path.contains("implementation_plan.md") {
            ChunkSourceKind::Plan
        } else if path.contains("/tests/") || path.contains("\\tests\\") || looks_like_test_body {
            ChunkSourceKind::Test
        } else if matches!(ext, "md" | "txt") {
            ChunkSourceKind::Documentation
        } else if matches!(ext, "toml" | "json" | "yaml" | "yml" | "lock") {
            ChunkSourceKind::Config
        } else if path.starts_with("demo://") {
            ChunkSourceKind::Generated
        } else {
            ChunkSourceKind::Implementation
        }
    }

    pub fn normalize_chunk_source_kinds(&mut self) -> bool {
        let mut changed = false;
        for chunk in &mut self.chunks {
            let inferred = Self::infer_chunk_source_kind(&chunk.source_path, &chunk.text);
            if chunk.source_kind != inferred {
                chunk.source_kind = inferred;
                changed = true;
            }
        }
        changed
    }

    pub fn normalize_chunk_memory_paths(&mut self, graph: &SpaceGraph) -> bool {
        let mut changed = false;
        for chunk in &mut self.chunks {
            let expected = chunk
                .space_ids
                .iter()
                .map(|space_id| (space_id.clone(), graph.path_labels(space_id)))
                .collect::<BTreeMap<_, _>>();
            if chunk.space_paths != expected {
                chunk.space_paths = expected;
                changed = true;
            }
        }
        changed
    }

    pub fn summary_json(&self) -> String {
        json_object([
            ("version".to_string(), self.version.to_string()),
            ("records".to_string(), self.records.len().to_string()),
            ("chunks".to_string(), self.chunks.len().to_string()),
            (
                "full_text_terms".to_string(),
                self.full_text.postings.len().to_string(),
            ),
            (
                "vector_chunks".to_string(),
                self.vector.chunks.len().to_string(),
            ),
            (
                "vector_dimensions".to_string(),
                self.vector.dimensions.to_string(),
            ),
        ])
    }

    pub fn inspect_json(&self) -> String {
        let mut chunks_by_space = std::collections::BTreeMap::new();
        let mut chunks_by_source = std::collections::BTreeMap::new();

        for chunk in &self.chunks {
            *chunks_by_source
                .entry(chunk.source_path.clone())
                .or_insert(0usize) += 1;
            for space in &chunk.space_ids {
                *chunks_by_space.entry(space.clone()).or_insert(0usize) += 1;
            }
        }

        let mut top_sources = chunks_by_source.into_iter().collect::<Vec<_>>();
        top_sources.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let mut top_spaces = chunks_by_space.into_iter().collect::<Vec<_>>();
        top_spaces.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

        let top_sources = top_sources.into_iter().take(8).map(|(source, count)| {
            json_object([
                ("source_path".to_string(), quote(&source)),
                ("chunk_count".to_string(), count.to_string()),
            ])
        });
        let top_spaces = top_spaces.into_iter().take(8).map(|(space, count)| {
            json_object([
                ("space_id".to_string(), quote(&space)),
                ("chunk_count".to_string(), count.to_string()),
            ])
        });

        json_object([
            ("summary".to_string(), self.summary_json()),
            ("top_sources".to_string(), json_array(top_sources)),
            ("top_spaces".to_string(), json_array(top_spaces)),
        ])
    }
}
