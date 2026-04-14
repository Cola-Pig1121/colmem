use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::project::{ProjectIngestPolicy, ProjectScope};
use crate::record::{
    Chunk, FullTextIndex, IndexState, Record, RecordSourceType, TokenPosting, VectorChunk,
    VectorIndex,
};
use crate::standard::standard_space_graph;

const MAX_CHUNK_CHARS: usize = 700;
const MIN_CHUNK_CHARS: usize = 40;
const VECTOR_DIMENSIONS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestPolicy {
    pub skipped_dirs: BTreeSet<String>,
    pub allowed_extensions: BTreeSet<String>,
    pub skipped_file_names: BTreeSet<String>,
    pub skipped_path_fragments: Vec<String>,
}

impl Default for IngestPolicy {
    fn default() -> Self {
        ProjectIngestPolicy::default().into()
    }
}

impl From<ProjectIngestPolicy> for IngestPolicy {
    fn from(policy: ProjectIngestPolicy) -> Self {
        Self {
            skipped_dirs: policy.skipped_dirs,
            allowed_extensions: policy.allowed_extensions,
            skipped_file_names: policy.skipped_file_names,
            skipped_path_fragments: policy.skipped_path_fragments,
        }
    }
}

impl IngestPolicy {
    fn should_skip_dir_name(&self, name: &str) -> bool {
        self.skipped_dirs.contains(name)
    }

    fn allows_file(&self, path: &Path) -> bool {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        matches!(extension, Some(ext) if self.allowed_extensions.contains(&ext))
    }

    fn should_skip_file(&self, path: &Path) -> bool {
        let normalized = path.to_string_lossy().replace('\\', "/");
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();

        self.skipped_file_names.contains(file_name)
            || self
                .skipped_path_fragments
                .iter()
                .any(|fragment| normalized.contains(fragment))
    }
}

#[derive(Clone, Debug, Default)]
pub struct IngestSummary {
    pub records: usize,
    pub chunks: usize,
    pub skipped_files: usize,
}

#[derive(Clone, Debug)]
struct ChunkDraft {
    text: String,
    line_start: usize,
    line_end: usize,
}

fn stable_hash(input: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .filter(|token| token.len() > 2)
        .map(|token| token.to_string())
        .collect()
}

fn hash_index(value: &str, dimensions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
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

fn normalize_vector(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in values {
            *value /= norm;
        }
    }
}

fn vectorize_text(path: &str, text: &str, dimensions: usize) -> Vec<f32> {
    let mut values = vec![0.0f32; dimensions];
    for token in tokenize(&format!("{path} {text}")) {
        let index = hash_index(&token, dimensions);
        values[index] += 1.0;
    }
    for trigram in char_ngrams(text) {
        let index = hash_index(&format!("tri:{trigram}"), dimensions);
        values[index] += 0.35;
    }
    normalize_vector(&mut values);
    values
}

fn infer_spaces(path: &Path, project: &ProjectScope, content: &str) -> BTreeSet<String> {
    let mut spaces = BTreeSet::new();
    spaces.insert("workspace_root".to_string());

    let path_text = path.to_string_lossy().to_ascii_lowercase();
    let content_text = content[..content.len().min(600)].to_ascii_lowercase();

    if path_text.contains("host") || path_text.contains("cursor") || path_text.contains("codex") {
        spaces.insert("host_adapters".to_string());
    }
    if path_text.contains("mcp") || path_text.contains("agent") || path_text.contains("cli") {
        spaces.insert("agent_runtime".to_string());
    }
    if path_text.contains("retriev")
        || content_text.contains("retriev")
        || content_text.contains("search")
        || content_text.contains("chunk")
        || content_text.contains("index")
    {
        spaces.insert("retrieval".to_string());
    }
    if path_text.contains("fact")
        || content_text.contains("fact")
        || content_text.contains("entity")
    {
        spaces.insert("facts".to_string());
    }
    if path_text.contains("readme")
        || path_text.contains("cargo.toml")
        || content_text.contains("architecture")
        || content_text.contains("workspace")
    {
        spaces.insert("architecture".to_string());
    }

    if spaces.len() == 1 {
        if project.focus_spaces.contains("architecture") {
            spaces.insert("architecture".to_string());
        } else if let Some(primary_focus) = project.focus_spaces.iter().next() {
            spaces.insert(primary_focus.clone());
        }
    }

    spaces
}

fn chunk_text(text: &str) -> Vec<ChunkDraft> {
    let mut paragraphs = Vec::new();
    let mut current_lines = Vec::new();
    let mut current_start = 1usize;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        if raw_line.trim().is_empty() {
            if !current_lines.is_empty() {
                paragraphs.push(ChunkDraft {
                    text: current_lines.join("\n").trim().to_string(),
                    line_start: current_start,
                    line_end: line_number.saturating_sub(1),
                });
                current_lines.clear();
            }
            current_start = line_number + 1;
            continue;
        }

        if current_lines.is_empty() {
            current_start = line_number;
        }
        current_lines.push(raw_line.to_string());
    }

    if !current_lines.is_empty() {
        paragraphs.push(ChunkDraft {
            text: current_lines.join("\n").trim().to_string(),
            line_start: current_start,
            line_end: text.lines().count().max(current_start),
        });
    }

    let mut chunks = Vec::new();
    let mut current_text = String::new();
    let mut chunk_start = 1usize;
    let mut chunk_end = 1usize;

    for paragraph in paragraphs {
        if paragraph.text.is_empty() {
            continue;
        }

        let candidate_len = if current_text.is_empty() {
            paragraph.text.len()
        } else {
            current_text.len() + paragraph.text.len() + 2
        };
        if candidate_len > MAX_CHUNK_CHARS && current_text.len() >= MIN_CHUNK_CHARS {
            chunks.push(ChunkDraft {
                text: current_text.trim().to_string(),
                line_start: chunk_start,
                line_end: chunk_end,
            });
            current_text.clear();
        }

        if current_text.is_empty() {
            chunk_start = paragraph.line_start;
        } else {
            current_text.push_str("\n\n");
        }
        current_text.push_str(&paragraph.text);
        chunk_end = paragraph.line_end;
    }

    if current_text.len() >= MIN_CHUNK_CHARS {
        chunks.push(ChunkDraft {
            text: current_text.trim().to_string(),
            line_start: chunk_start,
            line_end: chunk_end,
        });
    }

    if chunks.is_empty() && !text.trim().is_empty() {
        chunks.push(ChunkDraft {
            text: text.trim().chars().take(MAX_CHUNK_CHARS).collect(),
            line_start: 1,
            line_end: text.lines().count().max(1),
        });
    }

    chunks
}

fn collect_files(
    root: &Path,
    policy: &IngestPolicy,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| err.to_string())?;

        if file_type.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if policy.should_skip_dir_name(name) {
                continue;
            }
            collect_files(&path, policy, files)?;
        } else if file_type.is_file()
            && policy.allows_file(&path)
            && !policy.should_skip_file(&path)
        {
            files.push(path);
        }
    }
    Ok(())
}

pub fn build_project_index(project: &ProjectScope) -> Result<(IndexState, IngestSummary), String> {
    build_project_index_with_policy(project, &project.ingest_policy.clone().into())
}

pub fn build_project_index_with_policy(
    project: &ProjectScope,
    policy: &IngestPolicy,
) -> Result<(IndexState, IngestSummary), String> {
    let root = PathBuf::from(&project.root_path);
    if !root.exists() {
        return Err(format!(
            "project root does not exist: {}",
            project.root_path
        ));
    }

    let mut paths = Vec::new();
    collect_files(&root, policy, &mut paths)?;

    let mut index = IndexState {
        version: 1,
        full_text: FullTextIndex {
            version: 1,
            postings: BTreeMap::new(),
        },
        vector: VectorIndex {
            version: 1,
            dimensions: VECTOR_DIMENSIONS,
            chunks: Vec::new(),
        },
        records: Vec::new(),
        chunks: Vec::new(),
    };
    let mut summary = IngestSummary::default();

    for path in paths {
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => {
                summary.skipped_files += 1;
                continue;
            }
        };
        if content.trim().is_empty() {
            summary.skipped_files += 1;
            continue;
        }

        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let updated_at = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs().to_string())
            .unwrap_or_else(|| "0".to_string());
        let record_id = format!(
            "record-{}",
            stable_hash(&format!("{}:{updated_at}", relative))
        );
        let content_hash = stable_hash(&content);

        index.records.push(Record {
            id: record_id.clone(),
            project_id: project.id.clone(),
            source_type: RecordSourceType::ProjectFile,
            source_path: relative.clone(),
            created_at: updated_at.clone(),
            updated_at,
            content_hash,
            content: content.clone(),
        });
        summary.records += 1;

        let spaces = infer_spaces(&path, project, &content);
        let standard_graph = standard_space_graph();
        let space_paths = spaces
            .iter()
            .map(|space_id| (space_id.clone(), standard_graph.path_labels(space_id)))
            .collect::<BTreeMap<_, _>>();
        let source_kind = IndexState::infer_chunk_source_kind(&relative, &content);
        for (ordinal, chunk) in chunk_text(&content).into_iter().enumerate() {
            let chunk_hash = stable_hash(&format!(
                "{record_id}:{ordinal}:{}:{}:{}",
                chunk.line_start, chunk.line_end, chunk.text
            ));
            index.chunks.push(Chunk {
                id: format!("chunk-{chunk_hash}"),
                record_id: record_id.clone(),
                project_id: project.id.clone(),
                source_path: relative.clone(),
                source_kind: source_kind.clone(),
                ordinal,
                line_start: chunk.line_start,
                line_end: chunk.line_end,
                char_count: chunk.text.chars().count(),
                text: chunk.text,
                space_ids: spaces.clone(),
                space_paths: space_paths.clone(),
                hash: chunk_hash,
            });
            summary.chunks += 1;
        }
    }

    let mut postings = BTreeMap::<String, Vec<TokenPosting>>::new();
    for chunk in &index.chunks {
        let mut frequencies = BTreeMap::<String, u16>::new();
        for token in tokenize(&format!("{} {}", chunk.source_path, chunk.text)) {
            let entry = frequencies.entry(token).or_insert(0);
            *entry = entry.saturating_add(1);
        }
        for (token, frequency) in frequencies {
            postings.entry(token).or_default().push(TokenPosting {
                chunk_id: chunk.id.clone(),
                frequency,
            });
        }
    }
    index.full_text.postings = postings;
    index.vector.chunks = index
        .chunks
        .iter()
        .map(|chunk| VectorChunk {
            chunk_id: chunk.id.clone(),
            values: vectorize_text(&chunk.source_path, &chunk.text, VECTOR_DIMENSIONS),
        })
        .collect();

    Ok((index, summary))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::project::ProjectScope;
    use crate::record::ChunkSourceKind;

    use super::{
        IngestPolicy, VECTOR_DIMENSIONS, build_project_index, build_project_index_with_policy,
    };

    fn temp_dir() -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("colmem-ingest-test-{stamp}"));
        root
    }

    #[test]
    fn build_project_index_creates_records_and_chunks() {
        let root = temp_dir();
        fs::create_dir_all(root.join("src")).expect("create dirs");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn search() {\n    // retrieval pipeline\n}\n\nThis module handles hybrid retrieval for agents.",
        )
        .expect("write file");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, summary) = build_project_index(&project).expect("build index");

        assert_eq!(summary.records, 1);
        assert!(!index.chunks.is_empty());
        assert!(
            index
                .chunks
                .iter()
                .any(|chunk| chunk.space_ids.contains("retrieval"))
        );
        assert!(index.chunks.iter().all(|chunk| chunk.line_start >= 1));
        assert!(
            index
                .chunks
                .iter()
                .all(|chunk| chunk.line_end >= chunk.line_start)
        );
        assert!(
            index
                .records
                .iter()
                .all(|record| !record.content_hash.is_empty())
        );
        assert!(!index.full_text.postings.is_empty());
        assert_eq!(index.vector.dimensions, VECTOR_DIMENSIONS);
        assert_eq!(index.vector.chunks.len(), index.chunks.len());
    }

    #[test]
    fn build_project_index_persists_explicit_source_kinds() {
        let root = temp_dir();
        fs::create_dir_all(root.join("tests")).expect("create test dir");
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(root.join("src.rs"), "pub fn impl_only() {}\n").expect("write impl file");
        fs::write(
            root.join("tests/retrieval.rs"),
            "#[test]\nfn retrieval_test() { assert_eq!(1, 1); }\n",
        )
        .expect("write test file");
        fs::write(root.join("docs/guide.md"), "# Guide\n").expect("write docs file");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n")
            .expect("write config file");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, _) = build_project_index(&project).expect("build index");

        assert!(
            index
                .chunks
                .iter()
                .any(|chunk| chunk.source_kind == ChunkSourceKind::Implementation)
        );
        assert!(
            index
                .chunks
                .iter()
                .any(|chunk| chunk.source_kind == ChunkSourceKind::Test)
        );
        assert!(
            index
                .chunks
                .iter()
                .any(|chunk| chunk.source_kind == ChunkSourceKind::Documentation)
        );
        assert!(
            index
                .chunks
                .iter()
                .any(|chunk| chunk.source_kind == ChunkSourceKind::Config)
        );
    }

    #[test]
    fn build_project_index_applies_default_corpus_hygiene_skips() {
        let root = temp_dir();
        fs::create_dir_all(root.join("docs").join("04-dev-notes")).expect("create notes dir");
        fs::write(root.join("src.rs"), "pub fn keep_code() {}\n").expect("write code file");
        fs::write(root.join("IMPLEMENTATION_PLAN.md"), "# plan\n").expect("write plan");
        fs::write(root.join("ISSUES_TODO.md"), "# todo\n").expect("write todo");
        fs::write(
            root.join("docs").join("04-dev-notes").join("pitfalls.md"),
            "# notes\n",
        )
        .expect("write notes");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let (index, summary) = build_project_index(&project).expect("build index");

        assert_eq!(summary.records, 1);
        assert!(
            index
                .records
                .iter()
                .all(|record| !record.source_path.contains("IMPLEMENTATION_PLAN.md"))
        );
        assert!(
            index
                .records
                .iter()
                .all(|record| !record.source_path.contains("ISSUES_TODO.md"))
        );
        assert!(
            index
                .records
                .iter()
                .all(|record| !record.source_path.contains("04-dev-notes"))
        );
    }

    #[test]
    fn build_project_index_allows_custom_policy_to_include_default_skips() {
        let root = temp_dir();
        fs::create_dir_all(root.join("docs").join("04-dev-notes")).expect("create notes dir");
        fs::write(root.join("src.rs"), "pub fn keep_code() {}\n").expect("write code file");
        fs::write(root.join("IMPLEMENTATION_PLAN.md"), "# plan\n").expect("write plan");
        fs::write(root.join("ISSUES_TODO.md"), "# todo\n").expect("write todo");
        fs::write(
            root.join("docs").join("04-dev-notes").join("pitfalls.md"),
            "# notes\n",
        )
        .expect("write notes");

        let project = ProjectScope::new("demo", "Demo", root.display().to_string());
        let mut policy = IngestPolicy::default();
        policy.skipped_file_names.clear();
        policy.skipped_path_fragments.clear();

        let (index, summary) =
            build_project_index_with_policy(&project, &policy).expect("build index");

        assert_eq!(summary.records, 4);
        assert!(
            index
                .records
                .iter()
                .any(|record| record.source_path.contains("IMPLEMENTATION_PLAN.md"))
        );
        assert!(
            index
                .records
                .iter()
                .any(|record| record.source_path.contains("ISSUES_TODO.md"))
        );
        assert!(
            index
                .records
                .iter()
                .any(|record| record.source_path.contains("04-dev-notes"))
        );
    }

    #[test]
    fn build_project_index_uses_persisted_project_ingest_policy() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(
            root.join("IMPLEMENTATION_PLAN.md"),
            "# Plan\nThis plan should be indexed when project policy allows it.\n",
        )
        .expect("write plan file");

        let mut project = ProjectScope::new("demo", "Demo", root.display().to_string());
        project.ingest_policy.skipped_file_names.clear();

        let (index, _) = build_project_index(&project).expect("build index");

        assert!(
            index
                .records
                .iter()
                .any(|record| record.source_path.contains("IMPLEMENTATION_PLAN.md"))
        );
    }
}
