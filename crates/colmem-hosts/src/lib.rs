use std::collections::BTreeSet;

use colmem_core::host::{HostAdapter, HostDescriptor};
use colmem_core::model::{CapabilityKind, HostId, TransportKind};
use colmem_core::utils::{json_array, json_object, quote};

macro_rules! host_adapter {
    ($name:ident, $id:expr, $display:expr, $transport:expr, $stateful:expr, [$($kind:expr),* $(,)?], $hint:expr) => {
        pub struct $name;

        impl HostAdapter for $name {
            fn descriptor(&self) -> HostDescriptor {
                HostDescriptor {
                    id: $id,
                    display_name: $display,
                    transport: $transport,
                    supports_stateful_plugins: $stateful,
                    supported_capability_kinds: BTreeSet::from([$($kind),*]),
                    install_hint: $hint,
                }
            }
        }
    };
}

host_adapter!(
    ClaudeCodeAdapter,
    HostId::ClaudeCode,
    "Claude Code",
    TransportKind::Cli,
    true,
    [
        CapabilityKind::Skill,
        CapabilityKind::Tool,
        CapabilityKind::Plugin,
        CapabilityKind::McpEndpoint
    ],
    "Install colmem as a CLI plugin or attach the MCP server to Claude Code."
);
host_adapter!(
    CodexAdapter,
    HostId::Codex,
    "Codex",
    TransportKind::Cli,
    true,
    [
        CapabilityKind::Skill,
        CapabilityKind::Tool,
        CapabilityKind::Plugin,
        CapabilityKind::McpEndpoint
    ],
    "Use the colmem CLI locally or wire the MCP transport into Codex."
);
host_adapter!(
    CursorAdapter,
    HostId::Cursor,
    "Cursor",
    TransportKind::Cli,
    true,
    [
        CapabilityKind::Skill,
        CapabilityKind::Tool,
        CapabilityKind::Plugin,
        CapabilityKind::McpEndpoint
    ],
    "Attach colmem through the Cursor plugin bridge or stdio MCP."
);
host_adapter!(
    TraeIdeAdapter,
    HostId::TraeIde,
    "Trae IDE",
    TransportKind::Cli,
    false,
    [
        CapabilityKind::Skill,
        CapabilityKind::Tool,
        CapabilityKind::McpEndpoint
    ],
    "Use the CLI integration first; enable plugins only when the host allows them."
);
host_adapter!(
    OpenClawAdapter,
    HostId::OpenClaw,
    "OpenClaw",
    TransportKind::StdioMcp,
    true,
    [
        CapabilityKind::Skill,
        CapabilityKind::Tool,
        CapabilityKind::McpEndpoint
    ],
    "Attach the stdio MCP server and let OpenClaw discover colmem tools."
);

pub fn builtin_hosts() -> Vec<HostDescriptor> {
    vec![
        ClaudeCodeAdapter.descriptor(),
        CodexAdapter.descriptor(),
        CursorAdapter.descriptor(),
        TraeIdeAdapter.descriptor(),
        OpenClawAdapter.descriptor(),
    ]
}

pub fn find_host(id: &HostId) -> Option<HostDescriptor> {
    builtin_hosts()
        .into_iter()
        .find(|descriptor| &descriptor.id == id)
}

#[derive(Clone, Debug)]
pub struct HostAcceptanceStep {
    pub id: String,
    pub runner: String,
    pub action: String,
    pub payload: String,
    pub request_json: String,
    pub expected: String,
}

impl HostAcceptanceStep {
    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(&self.id)),
            ("runner".to_string(), quote(&self.runner)),
            ("action".to_string(), quote(&self.action)),
            ("payload".to_string(), quote(&self.payload)),
            ("request_json".to_string(), quote(&self.request_json)),
            ("expected".to_string(), quote(&self.expected)),
        ])
    }

    pub fn to_check_text(&self) -> String {
        format!(
            "{} via {} {} payload={} -> {}",
            self.id, self.runner, self.action, self.payload, self.expected
        )
    }
}

#[derive(Clone, Debug)]
pub struct HostInstallPlan {
    pub host: HostDescriptor,
    pub workspace_root: String,
    pub command: String,
    pub config_target: String,
    pub config_format: String,
    pub config_snippet: String,
    pub diagnostics: Vec<String>,
    pub acceptance_checks: Vec<String>,
    pub acceptance_plan: Vec<HostAcceptanceStep>,
}

impl HostInstallPlan {
    pub fn to_json(&self) -> String {
        json_object([
            ("host".to_string(), self.host.to_json()),
            ("workspace_root".to_string(), quote(&self.workspace_root)),
            ("command".to_string(), quote(&self.command)),
            ("config_target".to_string(), quote(&self.config_target)),
            ("config_format".to_string(), quote(&self.config_format)),
            ("config_snippet".to_string(), quote(&self.config_snippet)),
            (
                "diagnostics".to_string(),
                json_array(self.diagnostics.iter().map(|diagnostic| quote(diagnostic))),
            ),
            (
                "acceptance_checks".to_string(),
                json_array(self.acceptance_checks.iter().map(|check| quote(check))),
            ),
            (
                "acceptance_plan".to_string(),
                json_array(self.acceptance_plan.iter().map(|step| step.to_json())),
            ),
        ])
    }
}

fn mcp_server_json(workspace_root: &str) -> String {
    format!(
        "{{\"mcpServers\":{{\"colmem\":{{\"command\":\"colmem\",\"args\":[\"mcp\",\"serve\"],\"cwd\":\"{}\"}}}}}}",
        workspace_root.replace('\\', "\\\\")
    )
}

fn cli_plugin_json(workspace_root: &str) -> String {
    format!(
        "{{\"colmem\":{{\"command\":\"colmem\",\"args\":[\"mcp\",\"serve\"],\"cwd\":\"{}\"}}}}",
        workspace_root.replace('\\', "\\\\")
    )
}

fn codex_toml(workspace_root: &str) -> String {
    format!(
        "[mcp_servers.colmem]\ncommand = \"colmem\"\nargs = [\"mcp\", \"serve\"]\ncwd = \"{}\"",
        workspace_root.replace('\\', "\\\\")
    )
}

fn config_template_for_host(
    host: &HostDescriptor,
    workspace_root: &str,
) -> (&'static str, &'static str, String) {
    match host.id {
        HostId::ClaudeCode => (
            "Claude Code MCP server settings",
            "json:mcpServers",
            mcp_server_json(workspace_root),
        ),
        HostId::Codex => (
            "Codex MCP server configuration",
            "toml:mcp_servers",
            codex_toml(workspace_root),
        ),
        HostId::Cursor => (
            "Cursor MCP server configuration",
            "json:mcpServers",
            mcp_server_json(workspace_root),
        ),
        HostId::TraeIde => (
            "Trae IDE tool bridge or MCP server configuration",
            "json:cli_plugin",
            cli_plugin_json(workspace_root),
        ),
        HostId::OpenClaw => (
            "OpenClaw stdio MCP server configuration",
            "json:mcpServers",
            mcp_server_json(workspace_root),
        ),
        HostId::GenericMcp => (
            "Generic MCP client configuration",
            "json:mcpServers",
            mcp_server_json(workspace_root),
        ),
    }
}

fn acceptance_checks_for_host(host: &HostDescriptor, workspace_root: &str) -> Vec<String> {
    acceptance_plan_for_host(host, workspace_root)
        .into_iter()
        .map(|step| step.to_check_text())
        .collect()
}

fn acceptance_plan_for_host(
    host: &HostDescriptor,
    workspace_root: &str,
) -> Vec<HostAcceptanceStep> {
    vec![
        HostAcceptanceStep {
            id: "host_diagnostics".to_string(),
            runner: "cli".to_string(),
            action: "colmem host diagnostics".to_string(),
            payload: format!("{} {}", host.id.as_str(), workspace_root),
            request_json: String::new(),
            expected: "prints transport, config format, launch command, expected tools, and safety diagnostics".to_string(),
        },
        HostAcceptanceStep {
            id: "mcp_launch".to_string(),
            runner: "process".to_string(),
            action: "colmem mcp serve".to_string(),
            payload: format!("cwd={workspace_root}"),
            request_json: String::new(),
            expected: "stdio MCP server accepts JSON-RPC framed requests".to_string(),
        },
        HostAcceptanceStep {
            id: "mcp_tools_list".to_string(),
            runner: "mcp".to_string(),
            action: "tools/list".to_string(),
            payload: "{}".to_string(),
            request_json:
                "{\"jsonrpc\":\"2.0\",\"id\":\"host-smoke-tools\",\"method\":\"tools/list\"}"
                    .to_string(),
            expected:
                "returns colmem_query_plan, colmem_agent_inspect, colmem_capability_list, and colmem_memory_map"
                    .to_string(),
        },
        HostAcceptanceStep {
            id: "query_plan".to_string(),
            runner: "mcp".to_string(),
            action: "tools/call".to_string(),
            payload: format!(
                "{{\"name\":\"colmem_query_plan\",\"arguments\":{{\"query\":\"project status\",\"host\":\"{}\"}}}}",
                host.id.as_str()
            ),
            request_json: format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":\"host-smoke-query\",\"method\":\"tools/call\",\"params\":{{\"name\":\"colmem_query_plan\",\"arguments\":{{\"query\":\"project status\",\"host\":\"{}\"}}}}}}",
                host.id.as_str()
            ),
            expected: "returns structured query context and selected capability audit".to_string(),
        },
        HostAcceptanceStep {
            id: "agent_inspect".to_string(),
            runner: "mcp".to_string(),
            action: "tools/call".to_string(),
            payload: "{\"name\":\"colmem_agent_inspect\",\"arguments\":{\"agent\":\"builder\"}}"
                .to_string(),
            request_json:
                "{\"jsonrpc\":\"2.0\",\"id\":\"host-smoke-agent\",\"method\":\"tools/call\",\"params\":{\"name\":\"colmem_agent_inspect\",\"arguments\":{\"agent\":\"builder\"}}}"
                    .to_string(),
            expected: "returns the builder agent profile".to_string(),
        },
        HostAcceptanceStep {
            id: "capability_list".to_string(),
            runner: "mcp".to_string(),
            action: "tools/call".to_string(),
            payload: format!(
                "{{\"name\":\"colmem_capability_list\",\"arguments\":{{\"host\":\"{}\"}}}}",
                host.id.as_str()
            ),
            request_json: format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":\"host-smoke-capabilities\",\"method\":\"tools/call\",\"params\":{{\"name\":\"colmem_capability_list\",\"arguments\":{{\"host\":\"{}\"}}}}}}",
                host.id.as_str()
            ),
            expected:
                "returns capability compatibility diagnostics and selected_capabilities.audit"
                    .to_string(),
        },
        HostAcceptanceStep {
            id: "memory_map".to_string(),
            runner: "mcp".to_string(),
            action: "tools/call".to_string(),
            payload: "{\"name\":\"colmem_memory_map\",\"arguments\":{}}".to_string(),
            request_json:
                "{\"jsonrpc\":\"2.0\",\"id\":\"host-smoke-memory-map\",\"method\":\"tools/call\",\"params\":{\"name\":\"colmem_memory_map\",\"arguments\":{}}}"
                    .to_string(),
            expected: "returns structured memory map nodes, links, and memory paths".to_string(),
        },
    ]
}

pub fn install_plan_for_host(
    host: &HostDescriptor,
    workspace_root: impl Into<String>,
) -> HostInstallPlan {
    let workspace_root = workspace_root.into();
    let command = "colmem mcp serve".to_string();
    let (config_target, config_format, config_snippet) =
        config_template_for_host(host, &workspace_root);
    let acceptance_plan = acceptance_plan_for_host(host, &workspace_root);
    let acceptance_checks = acceptance_checks_for_host(host, &workspace_root);

    let mut diagnostics = vec![
        format!("transport={}", host.transport.as_str()),
        format!("config_format={config_format}"),
        format!("stateful_plugins={}", host.supports_stateful_plugins),
        format!("launch_command={command}"),
        "expected_tools=colmem_query_plan,colmem_agent_inspect,colmem_capability_list,colmem_memory_map".to_string(),
        format!("acceptance_checks={}", acceptance_checks.len()),
    ];
    if !host.supports_kind(&CapabilityKind::Plugin) {
        diagnostics.push("plugins_not_supported_by_host_descriptor".to_string());
    }
    if host.transport != TransportKind::StdioMcp {
        diagnostics.push(
            "host_descriptor_uses_cli_transport; prefer stdio MCP config where supported"
                .to_string(),
        );
    }

    HostInstallPlan {
        host: host.clone(),
        workspace_root,
        command,
        config_target: config_target.to_string(),
        config_format: config_format.to_string(),
        config_snippet,
        diagnostics,
        acceptance_checks,
        acceptance_plan,
    }
}

pub fn install_plan_for_host_id(
    host_id: &HostId,
    workspace_root: impl Into<String>,
) -> Result<HostInstallPlan, String> {
    let host = find_host(host_id).ok_or_else(|| format!("unknown host: {}", host_id.as_str()))?;
    Ok(install_plan_for_host(&host, workspace_root))
}

#[cfg(test)]
mod tests {
    use colmem_core::model::HostId;

    use super::{builtin_hosts, install_plan_for_host_id};

    #[test]
    fn builtin_catalog_includes_expected_hosts() {
        let hosts = builtin_hosts();
        assert!(hosts.iter().any(|host| host.display_name == "Claude Code"));
        assert!(hosts.iter().any(|host| host.display_name == "Codex"));
    }

    #[test]
    fn install_plan_includes_mcp_launch_command_and_workspace() {
        let plan = install_plan_for_host_id(&HostId::OpenClaw, "D:/repo/colmem").expect("plan");

        assert_eq!(plan.command, "colmem mcp serve");
        assert!(plan.config_snippet.contains("mcpServers"));
        assert!(plan.config_snippet.contains("D:/repo/colmem"));
        assert_eq!(plan.config_format, "json:mcpServers");
        assert!(
            plan.acceptance_checks
                .iter()
                .any(|check| check.contains("colmem_query_plan"))
        );
        assert!(
            plan.acceptance_plan
                .iter()
                .any(|step| step.id == "mcp_tools_list"
                    && step.action == "tools/list"
                    && step.request_json.contains("\"method\":\"tools/list\""))
        );
        assert!(
            plan.diagnostics
                .iter()
                .any(|diagnostic| diagnostic == "transport=stdio_mcp")
        );
    }

    #[test]
    fn install_plan_uses_codex_toml_template() {
        let plan = install_plan_for_host_id(&HostId::Codex, "D:/repo/colmem").expect("plan");

        assert_eq!(plan.config_format, "toml:mcp_servers");
        assert!(plan.config_snippet.contains("[mcp_servers.colmem]"));
        assert!(
            plan.diagnostics
                .iter()
                .any(|diagnostic| diagnostic == "config_format=toml:mcp_servers")
        );
    }
}
