use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{AgentRole, CapabilityKind, HostId, TaskKind};
use crate::project::ProjectScope;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BindingMode {
    Auto,
    ForceEnabled,
    ForceDisabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityPermission {
    Read,
    Write,
    Stdio,
    Unknown(String),
}

impl CapabilityPermission {
    pub fn parse(value: &str) -> Self {
        match value {
            "read" => Self::Read,
            "write" => Self::Write,
            "stdio" => Self::Stdio,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Stdio => "stdio",
            Self::Unknown(value) => value.as_str(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub id: String,
    pub kind: CapabilityKind,
    pub provider: String,
    pub version: String,
    pub summary: String,
    pub compatible_hosts: BTreeSet<HostId>,
    pub compatible_roles: BTreeSet<AgentRole>,
    pub project_tags: BTreeSet<String>,
    pub permissions: Vec<String>,
    pub activation_hints: BTreeSet<TaskKind>,
    pub stateful: bool,
}

impl CapabilityDescriptor {
    pub fn matches_host(&self, host: &HostId) -> bool {
        self.compatible_hosts.is_empty() || self.compatible_hosts.contains(host)
    }

    pub fn matches_role(&self, role: &AgentRole) -> bool {
        self.compatible_roles.is_empty() || self.compatible_roles.contains(role)
    }

    pub fn matches_task(&self, task: &TaskKind) -> bool {
        self.activation_hints.is_empty() || self.activation_hints.contains(task)
    }

    pub fn matches_project(&self, project: &ProjectScope) -> bool {
        self.project_tags.is_empty()
            || self
                .project_tags
                .iter()
                .any(|tag| project.tags.contains(tag))
    }

    pub fn parsed_permissions(&self) -> Vec<CapabilityPermission> {
        self.permissions
            .iter()
            .map(|permission| CapabilityPermission::parse(permission))
            .collect()
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(&self.id)),
            ("kind".to_string(), quote(self.kind.as_str())),
            ("provider".to_string(), quote(&self.provider)),
            ("version".to_string(), quote(&self.version)),
            ("summary".to_string(), quote(&self.summary)),
            (
                "compatible_hosts".to_string(),
                json_array(
                    self.compatible_hosts
                        .iter()
                        .map(|host| quote(host.as_str())),
                ),
            ),
            (
                "compatible_roles".to_string(),
                json_array(
                    self.compatible_roles
                        .iter()
                        .map(|role| quote(role.as_str())),
                ),
            ),
            (
                "project_tags".to_string(),
                json_array(self.project_tags.iter().map(|tag| quote(tag))),
            ),
            (
                "permissions".to_string(),
                json_array(self.permissions.iter().map(|permission| quote(permission))),
            ),
            (
                "activation_hints".to_string(),
                json_array(
                    self.activation_hints
                        .iter()
                        .map(|hint| quote(hint.as_str())),
                ),
            ),
            ("stateful".to_string(), self.stateful.to_string()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::CapabilityPermission;

    #[test]
    fn capability_permission_parse_preserves_unknown_values() {
        assert_eq!(
            CapabilityPermission::parse("read"),
            CapabilityPermission::Read
        );
        assert_eq!(
            CapabilityPermission::parse("write"),
            CapabilityPermission::Write
        );
        assert_eq!(
            CapabilityPermission::parse("stdio"),
            CapabilityPermission::Stdio
        );
        assert_eq!(CapabilityPermission::parse("network").as_str(), "network");
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    pub capabilities: BTreeMap<String, CapabilityDescriptor>,
}

impl CapabilityRegistry {
    pub fn register(&mut self, capability: CapabilityDescriptor) {
        self.capabilities.insert(capability.id.clone(), capability);
    }

    pub fn get(&self, id: &str) -> Option<&CapabilityDescriptor> {
        self.capabilities.get(id)
    }

    pub fn list(&self) -> Vec<&CapabilityDescriptor> {
        self.capabilities.values().collect()
    }

    pub fn to_json(&self) -> String {
        json_array(
            self.capabilities
                .values()
                .map(CapabilityDescriptor::to_json),
        )
    }
}
