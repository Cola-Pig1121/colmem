use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::HostId;
use crate::rerank::SourceWeightConfig;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectIngestPolicy {
    pub skipped_dirs: BTreeSet<String>,
    pub allowed_extensions: BTreeSet<String>,
    pub skipped_file_names: BTreeSet<String>,
    pub skipped_path_fragments: Vec<String>,
}

impl Default for ProjectIngestPolicy {
    fn default() -> Self {
        Self {
            skipped_dirs: BTreeSet::from([
                ".git".to_string(),
                ".colmem".to_string(),
                "target".to_string(),
                "node_modules".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "dist".to_string(),
                "build".to_string(),
            ]),
            allowed_extensions: BTreeSet::from([
                "rs".to_string(),
                "toml".to_string(),
                "md".to_string(),
                "txt".to_string(),
                "json".to_string(),
                "yml".to_string(),
                "yaml".to_string(),
                "ts".to_string(),
                "tsx".to_string(),
                "js".to_string(),
                "py".to_string(),
                "sh".to_string(),
                "lock".to_string(),
            ]),
            skipped_file_names: BTreeSet::from([
                "IMPLEMENTATION_PLAN.md".to_string(),
                "ISSUES_TODO.md".to_string(),
            ]),
            skipped_path_fragments: vec!["/docs/04-".to_string()],
        }
    }
}

impl ProjectIngestPolicy {
    pub fn to_json(&self) -> String {
        json_object([
            (
                "skipped_dirs".to_string(),
                json_array(self.skipped_dirs.iter().map(|value| quote(value))),
            ),
            (
                "allowed_extensions".to_string(),
                json_array(self.allowed_extensions.iter().map(|value| quote(value))),
            ),
            (
                "skipped_file_names".to_string(),
                json_array(self.skipped_file_names.iter().map(|value| quote(value))),
            ),
            (
                "skipped_path_fragments".to_string(),
                json_array(self.skipped_path_fragments.iter().map(|value| quote(value))),
            ),
        ])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectHostPolicy {
    pub disabled_capabilities: BTreeSet<String>,
    pub preferred_capabilities: BTreeSet<String>,
}

impl ProjectHostPolicy {
    pub fn to_json(&self) -> String {
        json_object([
            (
                "disabled_capabilities".to_string(),
                json_array(self.disabled_capabilities.iter().map(|id| quote(id))),
            ),
            (
                "preferred_capabilities".to_string(),
                json_array(self.preferred_capabilities.iter().map(|id| quote(id))),
            ),
        ])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectScope {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub tags: BTreeSet<String>,
    pub focus_spaces: BTreeSet<String>,
    pub required_capabilities: BTreeSet<String>,
    pub disabled_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub ingest_policy: ProjectIngestPolicy,
    #[serde(default)]
    pub rerank_source_weights: SourceWeightConfig,
    pub host_overrides: BTreeMap<HostId, ProjectHostPolicy>,
}

impl ProjectScope {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        root_path: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            root_path: root_path.into(),
            tags: BTreeSet::new(),
            focus_spaces: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
            disabled_capabilities: BTreeSet::new(),
            ingest_policy: ProjectIngestPolicy::default(),
            rerank_source_weights: SourceWeightConfig::default(),
            host_overrides: BTreeMap::new(),
        }
    }

    pub fn disabled_for_host(&self, host: &HostId) -> BTreeSet<String> {
        let mut disabled = self.disabled_capabilities.clone();
        if let Some(policy) = self.host_overrides.get(host) {
            disabled.extend(policy.disabled_capabilities.iter().cloned());
        }
        disabled
    }

    pub fn preferred_for_host(&self, host: &HostId) -> BTreeSet<String> {
        self.host_overrides
            .get(host)
            .map(|policy| policy.preferred_capabilities.clone())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(&self.id)),
            ("name".to_string(), quote(&self.name)),
            ("root_path".to_string(), quote(&self.root_path)),
            (
                "tags".to_string(),
                json_array(self.tags.iter().map(|tag| quote(tag))),
            ),
            (
                "focus_spaces".to_string(),
                json_array(self.focus_spaces.iter().map(|space| quote(space))),
            ),
            (
                "required_capabilities".to_string(),
                json_array(self.required_capabilities.iter().map(|id| quote(id))),
            ),
            (
                "disabled_capabilities".to_string(),
                json_array(self.disabled_capabilities.iter().map(|id| quote(id))),
            ),
            ("ingest_policy".to_string(), self.ingest_policy.to_json()),
            (
                "rerank_source_weights".to_string(),
                source_weight_config_json(&self.rerank_source_weights),
            ),
            (
                "host_overrides".to_string(),
                json_object(
                    self.host_overrides
                        .iter()
                        .map(|(host, policy)| (host.as_str().to_string(), policy.to_json())),
                ),
            ),
        ])
    }
}

fn source_weight_config_json(config: &SourceWeightConfig) -> String {
    json_object([
        (
            "implementation_default".to_string(),
            config.implementation_default.to_string(),
        ),
        (
            "implementation_review".to_string(),
            config.implementation_review.to_string(),
        ),
        (
            "implementation_refactor".to_string(),
            config.implementation_refactor.to_string(),
        ),
        (
            "implementation_diagnose".to_string(),
            config.implementation_diagnose.to_string(),
        ),
        (
            "test_preferred".to_string(),
            config.test_preferred.to_string(),
        ),
        ("test_generic".to_string(), config.test_generic.to_string()),
        (
            "documentation_preferred".to_string(),
            config.documentation_preferred.to_string(),
        ),
        (
            "documentation_generic".to_string(),
            config.documentation_generic.to_string(),
        ),
        (
            "config_preferred".to_string(),
            config.config_preferred.to_string(),
        ),
        (
            "config_generic".to_string(),
            config.config_generic.to_string(),
        ),
        (
            "plan_preferred".to_string(),
            config.plan_preferred.to_string(),
        ),
        ("plan_generic".to_string(), config.plan_generic.to_string()),
        (
            "generated_generic".to_string(),
            config.generated_generic.to_string(),
        ),
    ])
}
