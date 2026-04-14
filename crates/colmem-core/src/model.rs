use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum CapabilityKind {
    Skill,
    Tool,
    Plugin,
    McpEndpoint,
}

impl CapabilityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Tool => "tool",
            Self::Plugin => "plugin",
            Self::McpEndpoint => "mcp_endpoint",
        }
    }
}

impl Display for CapabilityKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum AgentRole {
    Architect,
    Builder,
    Researcher,
    Reviewer,
    Operator,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Architect => "architect",
            Self::Builder => "builder",
            Self::Researcher => "researcher",
            Self::Reviewer => "reviewer",
            Self::Operator => "operator",
        }
    }
}

impl Display for AgentRole {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentRole {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "architect" => Ok(Self::Architect),
            "builder" | "coder" => Ok(Self::Builder),
            "researcher" => Ok(Self::Researcher),
            "reviewer" => Ok(Self::Reviewer),
            "operator" | "ops" => Ok(Self::Operator),
            other => Err(format!("unknown agent role: {other}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum TaskKind {
    Query,
    Refactor,
    Diagnose,
    Review,
    Index,
    Serve,
}

impl TaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Refactor => "refactor",
            Self::Diagnose => "diagnose",
            Self::Review => "review",
            Self::Index => "index",
            Self::Serve => "serve",
        }
    }
}

impl Display for TaskKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum HostId {
    ClaudeCode,
    Codex,
    Cursor,
    TraeIde,
    OpenClaw,
    GenericMcp,
}

impl HostId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::TraeIde => "trae_ide",
            Self::OpenClaw => "openclaw",
            Self::GenericMcp => "generic_mcp",
        }
    }
}

impl Display for HostId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HostId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude_code" | "claudecode" | "claude" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "cursor" => Ok(Self::Cursor),
            "trae_ide" | "trae" => Ok(Self::TraeIde),
            "openclaw" => Ok(Self::OpenClaw),
            "generic_mcp" | "mcp" => Ok(Self::GenericMcp),
            other => Err(format!("unknown host: {other}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum TransportKind {
    Cli,
    StdioMcp,
}

impl TransportKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::StdioMcp => "stdio_mcp",
        }
    }
}

impl Display for TransportKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
