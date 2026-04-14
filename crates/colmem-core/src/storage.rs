use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{AgentProfile, EvolutionPatch, EvolutionSignal};
use crate::capability::CapabilityRegistry;
use crate::facts::InMemoryFactStore;
use crate::project::ProjectScope;
use crate::record::IndexState;
use crate::space::SpaceGraph;
use crate::standard::{
    standard_agents, standard_fact_store, standard_project, standard_registry, standard_space_graph,
};
use crate::utils::quote;

fn legacy_workspace_state_version() -> u32 {
    1
}

#[derive(Clone, Debug)]
pub struct WorkspacePaths {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub state_file: PathBuf,
}

impl WorkspacePaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let config_dir = root.join(".colmem");
        let state_file = config_dir.join("workspace-state.json");
        Self {
            root,
            config_dir,
            state_file,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionRecord {
    pub agent_id: String,
    pub reason: String,
    pub signal: EvolutionSignal,
    pub patch: EvolutionPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceState {
    #[serde(default = "legacy_workspace_state_version")]
    pub version: u32,
    pub workspace_name: String,
    #[serde(default)]
    pub projects: Vec<ProjectScope>,
    #[serde(default)]
    pub agents: Vec<AgentProfile>,
    #[serde(default)]
    pub registry: CapabilityRegistry,
    #[serde(default)]
    pub spaces: SpaceGraph,
    #[serde(default)]
    pub memory_paths: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub facts: InMemoryFactStore,
    #[serde(default)]
    pub index: IndexState,
    #[serde(default)]
    pub evolution_history: Vec<EvolutionRecord>,
}

pub const CURRENT_WORKSPACE_STATE_VERSION: u32 = 4;

impl WorkspaceState {
    pub fn bootstrap(root: &Path) -> Self {
        let mut state = Self {
            version: CURRENT_WORKSPACE_STATE_VERSION,
            workspace_name: "colmem".to_string(),
            projects: vec![standard_project(root.display().to_string())],
            agents: standard_agents(),
            registry: standard_registry(),
            spaces: standard_space_graph(),
            memory_paths: BTreeMap::new(),
            facts: standard_fact_store(),
            index: IndexState::default(),
            evolution_history: Vec::new(),
        };
        state.normalize_memory_paths();
        state.facts.ensure_audit_baseline();
        state
    }

    pub fn primary_project(&self) -> Option<&ProjectScope> {
        self.projects.first()
    }

    pub fn project_by_id(&self, id: &str) -> Option<&ProjectScope> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn project_by_id_mut(&mut self, id: &str) -> Option<&mut ProjectScope> {
        self.projects.iter_mut().find(|project| project.id == id)
    }

    pub fn agent_by_id(&self, id: &str) -> Option<&AgentProfile> {
        self.agents.iter().find(|agent| agent.id == id)
    }

    pub fn agent_by_id_mut(&mut self, id: &str) -> Option<&mut AgentProfile> {
        self.agents.iter_mut().find(|agent| agent.id == id)
    }

    pub fn upsert_project(&mut self, project: ProjectScope) {
        if let Some(existing) = self.project_by_id_mut(&project.id) {
            *existing = project;
        } else {
            self.projects.push(project);
        }
    }

    pub fn record_evolution(
        &mut self,
        agent_id: impl Into<String>,
        reason: impl Into<String>,
        signal: EvolutionSignal,
        patch: EvolutionPatch,
    ) {
        self.evolution_history.push(EvolutionRecord {
            agent_id: agent_id.into(),
            reason: reason.into(),
            signal,
            patch,
        });
    }

    pub fn to_pretty_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|err| err.to_string())
    }

    pub fn normalize_legacy_state(&mut self) -> bool {
        let mut changed = false;
        for fact in self.facts.all_mut() {
            for evidence_id in &mut fact.evidence_ids {
                let replacement = match evidence_id.as_str() {
                    "decision-001" => Some("path:crates/colmem-core/src/standard.rs"),
                    "decision-002" => Some("path:crates/colmem-core/src/retrieval.rs"),
                    "manual-colmem-supports-mcp" => Some("path:crates/colmem-core/src/mcp.rs"),
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    *evidence_id = replacement.to_string();
                    changed = true;
                }
            }
        }
        changed |= self.facts.merge_duplicate_facts();
        changed |= self.facts.ensure_audit_baseline();
        changed |= self.index.normalize_chunk_source_kinds();
        changed |= self.index.normalize_chunk_memory_paths(&self.spaces);
        changed |= self.normalize_memory_paths();
        changed
    }

    pub fn normalize_memory_paths(&mut self) -> bool {
        let expected = self.spaces.path_index();
        if self.memory_paths != expected {
            self.memory_paths = expected;
            true
        } else {
            false
        }
    }

    pub fn migrate_to_current(&mut self, loaded_version: u32) -> bool {
        let mut changed = self.normalize_legacy_state();
        if loaded_version != CURRENT_WORKSPACE_STATE_VERSION
            || self.version != CURRENT_WORKSPACE_STATE_VERSION
        {
            self.version = CURRENT_WORKSPACE_STATE_VERSION;
            changed = true;
        }
        changed
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceStateStore {
    pub paths: WorkspacePaths,
}

impl WorkspaceStateStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: WorkspacePaths::from_root(root),
        }
    }

    pub fn ensure_dir(&self) -> Result<(), String> {
        fs::create_dir_all(&self.paths.config_dir).map_err(|err| err.to_string())
    }

    pub fn save(&self, state: &WorkspaceState) -> Result<(), String> {
        self.ensure_dir()?;
        let mut persisted = state.clone();
        persisted.version = CURRENT_WORKSPACE_STATE_VERSION;
        let payload = persisted.to_pretty_json()?;
        fs::write(&self.paths.state_file, payload).map_err(|err| err.to_string())
    }

    pub fn load(&self) -> Result<WorkspaceState, String> {
        let contents = fs::read_to_string(&self.paths.state_file).map_err(|err| err.to_string())?;
        let raw_value: Value = serde_json::from_str(&contents).map_err(|err| err.to_string())?;
        let loaded_version = parse_workspace_state_version(&raw_value)?;
        if loaded_version > CURRENT_WORKSPACE_STATE_VERSION {
            return Err(format!(
                "workspace state version {} is newer than supported version {}",
                loaded_version, CURRENT_WORKSPACE_STATE_VERSION
            ));
        }
        let mut state: WorkspaceState =
            serde_json::from_value(raw_value).map_err(|err| err.to_string())?;
        let changed = state.migrate_to_current(loaded_version);
        if changed {
            self.save(&state)?;
        }
        Ok(state)
    }

    pub fn load_or_bootstrap(&self) -> Result<WorkspaceState, String> {
        if self.paths.state_file.exists() {
            self.load()
        } else {
            let state = WorkspaceState::bootstrap(&self.paths.root);
            self.save(&state)?;
            Ok(state)
        }
    }

    pub fn diagnostics_json(&self) -> String {
        format!(
            "{{\"root\": {}, \"config_dir\": {}, \"state_file\": {}, \"exists\": {}}}",
            quote(&self.paths.root.display().to_string()),
            quote(&self.paths.config_dir.display().to_string()),
            quote(&self.paths.state_file.display().to_string()),
            self.paths.state_file.exists()
        )
    }
}

fn parse_workspace_state_version(value: &Value) -> Result<u32, String> {
    let Some(raw_version) = value.get("version") else {
        return Ok(1);
    };

    let Some(version) = raw_version.as_u64() else {
        return Err("workspace state version must be an unsigned integer".to_string());
    };

    u32::try_from(version).map_err(|_| "workspace state version is too large".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    use super::{CURRENT_WORKSPACE_STATE_VERSION, WorkspaceState, WorkspaceStateStore};

    fn temp_dir() -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("colmem-storage-test-{stamp}"));
        root
    }

    use std::path::PathBuf;

    #[test]
    fn load_or_bootstrap_creates_state_file() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let state = store.load_or_bootstrap().expect("bootstrap state");
        assert!(store.paths.state_file.exists());
        assert_eq!(state.workspace_name, "colmem");
        assert_eq!(
            state
                .memory_paths
                .get("retrieval")
                .expect("retrieval memory path"),
            &vec![
                "Workspace Root".to_string(),
                "Architecture".to_string(),
                "Retrieval".to_string()
            ]
        );
    }

    #[test]
    fn save_and_reload_preserves_evolution_history() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let mut state = store.load_or_bootstrap().expect("bootstrap state");
        state.record_evolution(
            "builder",
            "test evolution",
            Default::default(),
            Default::default(),
        );
        store.save(&state).expect("save state");
        let reloaded = store.load().expect("reload state");
        assert_eq!(reloaded.evolution_history.len(), 1);
        assert_eq!(reloaded.evolution_history[0].agent_id, "builder");
    }

    #[test]
    fn load_or_bootstrap_upgrades_legacy_workspace_state_version() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let mut legacy_state = WorkspaceState::bootstrap(&root);
        legacy_state.version = 1;
        store.ensure_dir().expect("ensure dir");
        fs::write(
            &store.paths.state_file,
            serde_json::to_string_pretty(&legacy_state).expect("serialize legacy state"),
        )
        .expect("write legacy state");

        let upgraded = store.load_or_bootstrap().expect("upgrade state");
        assert_eq!(upgraded.version, CURRENT_WORKSPACE_STATE_VERSION);

        let persisted: Value = serde_json::from_str(
            &fs::read_to_string(&store.paths.state_file).expect("read persisted state"),
        )
        .expect("parse persisted state");
        assert_eq!(
            persisted
                .get("version")
                .and_then(Value::as_u64)
                .expect("persisted version"),
            u64::from(CURRENT_WORKSPACE_STATE_VERSION)
        );
        assert!(persisted.get("memory_paths").is_some());
    }

    #[test]
    fn load_migrates_missing_memory_paths() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let mut legacy_state = WorkspaceState::bootstrap(&root);
        legacy_state.version = 2;
        legacy_state.memory_paths.clear();
        legacy_state.index.chunks.push(crate::record::Chunk {
            id: "chunk-retrieval".to_string(),
            record_id: "record-retrieval".to_string(),
            project_id: "colmem".to_string(),
            source_path: "src/retrieval.rs".to_string(),
            source_kind: crate::record::ChunkSourceKind::Implementation,
            ordinal: 0,
            line_start: 1,
            line_end: 2,
            char_count: 20,
            text: "retrieval memory path".to_string(),
            space_ids: BTreeSet::from(["retrieval".to_string()]),
            space_paths: BTreeMap::new(),
            hash: "hash".to_string(),
        });
        store.ensure_dir().expect("ensure dir");
        fs::write(
            &store.paths.state_file,
            serde_json::to_string_pretty(&legacy_state).expect("serialize legacy state"),
        )
        .expect("write legacy state");

        let loaded = store.load().expect("load migrated state");
        assert_eq!(loaded.version, CURRENT_WORKSPACE_STATE_VERSION);
        assert_eq!(
            loaded
                .memory_paths
                .get("retrieval")
                .expect("retrieval memory path"),
            &vec![
                "Workspace Root".to_string(),
                "Architecture".to_string(),
                "Retrieval".to_string()
            ]
        );
        assert_eq!(
            loaded.index.chunks[0]
                .space_paths
                .get("retrieval")
                .expect("chunk retrieval memory path"),
            &vec![
                "Workspace Root".to_string(),
                "Architecture".to_string(),
                "Retrieval".to_string()
            ]
        );
    }

    #[test]
    fn load_rejects_newer_workspace_state_version() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let mut future_state = WorkspaceState::bootstrap(&root);
        future_state.version = CURRENT_WORKSPACE_STATE_VERSION + 1;
        store.ensure_dir().expect("ensure dir");
        fs::write(
            &store.paths.state_file,
            serde_json::to_string_pretty(&future_state).expect("serialize future state"),
        )
        .expect("write future state");

        let error = store.load().expect_err("future state should fail");
        assert!(error.contains("newer than supported version"));
    }

    #[test]
    fn load_or_bootstrap_accepts_legacy_state_without_version_field() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let state = WorkspaceState::bootstrap(&root);
        let mut raw_state: Value = serde_json::to_value(&state).expect("serialize workspace state");
        raw_state
            .as_object_mut()
            .expect("workspace state object")
            .remove("version");
        store.ensure_dir().expect("ensure dir");
        fs::write(
            &store.paths.state_file,
            serde_json::to_string_pretty(&raw_state).expect("serialize raw legacy state"),
        )
        .expect("write legacy state");

        let loaded = store.load_or_bootstrap().expect("load legacy state");
        assert_eq!(loaded.version, CURRENT_WORKSPACE_STATE_VERSION);

        let persisted: Value = serde_json::from_str(
            &fs::read_to_string(&store.paths.state_file).expect("read persisted state"),
        )
        .expect("parse persisted state");
        assert_eq!(
            persisted
                .get("version")
                .and_then(Value::as_u64)
                .expect("persisted version"),
            u64::from(CURRENT_WORKSPACE_STATE_VERSION)
        );
    }

    #[test]
    fn save_always_persists_current_workspace_state_version() {
        let root = temp_dir();
        let store = WorkspaceStateStore::new(&root);
        let mut state = WorkspaceState::bootstrap(&root);
        state.version = 1;
        store.save(&state).expect("save state");

        let persisted: Value = serde_json::from_str(
            &fs::read_to_string(&store.paths.state_file).expect("read persisted state"),
        )
        .expect("parse persisted state");
        assert_eq!(
            persisted
                .get("version")
                .and_then(Value::as_u64)
                .expect("persisted version"),
            u64::from(CURRENT_WORKSPACE_STATE_VERSION)
        );
    }
}
