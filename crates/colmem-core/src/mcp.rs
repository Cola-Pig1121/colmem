use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::{Value, json};

use crate::agent::{EvolutionPatch, EvolutionSignal};
use crate::facts::{FactQueryScope, InMemoryFactStore};
use crate::harness::TaskIntent;
use crate::host::{HostContext, HostDescriptor};
use crate::model::{CapabilityKind, HostId, TaskKind, TransportKind};
use crate::standard::standard_harness;
use crate::storage::WorkspaceStateStore;

fn result_response(id: Value, result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
    .to_string()
}

fn tool_result_response(id: Value, payload: Value) -> String {
    result_response(
        id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": payload.to_string(),
                }
            ],
            "structuredContent": payload,
        }),
    )
}

fn error_response(id: Value, code: i32, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
    .to_string()
}

fn invalid_params_response(id: Value, message: impl Into<String>) -> String {
    error_response(id, -32602, &message.into())
}

fn read_stdio_message<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut first_line = String::new();
    loop {
        first_line.clear();
        let bytes_read = reader.read_line(&mut first_line)?;
        if bytes_read == 0 {
            return Ok(None);
        }
        if !first_line.trim().is_empty() {
            break;
        }
    }

    if first_line.trim_start().starts_with('{') {
        return Ok(Some(first_line));
    }

    let mut content_length = None;
    let mut header_line = first_line;
    loop {
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            content_length = Some(value.trim().parse::<usize>().map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid Content-Length header: {err}"),
                )
            })?);
        }

        header_line = String::new();
        let bytes_read = reader.read_line(&mut header_line)?;
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading MCP headers",
            ));
        }
    }

    let content_length = content_length.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length header for MCP stdio message",
        )
    })?;
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body).map(Some).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid UTF-8 body: {err}"),
        )
    })
}

fn write_stdio_message<W: Write>(writer: &mut W, response: &str) -> io::Result<()> {
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        response.len(),
        response
    )?;
    writer.flush()
}

fn docs_root(default_root: &str) -> PathBuf {
    PathBuf::from(default_root).join("docs")
}

fn detect_resource_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "md" => "text/markdown",
        "base" => "text/yaml",
        "canvas" => "application/json",
        _ => "text/plain",
    }
}

fn resource_uri_for_relative(relative_path: &str) -> String {
    format!("colmem://docs/{relative_path}")
}

fn list_doc_resources(root: &Path) -> io::Result<Vec<Value>> {
    let docs_dir = docs_root(&root.display().to_string());
    if !docs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut resources = Vec::new();
    let mut stack = vec![docs_dir.clone()];
    while let Some(current_dir) = stack.pop() {
        for entry in fs::read_dir(&current_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default();
            if !matches!(extension, "md" | "base" | "canvas") {
                continue;
            }

            let relative = path
                .strip_prefix(&docs_dir)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&relative)
                .to_string();
            resources.push(json!({
                "uri": resource_uri_for_relative(&relative),
                "name": name,
                "description": format!("Docs resource: {relative}"),
                "mimeType": detect_resource_mime_type(&path)
            }));
        }
    }

    resources.sort_by(|left, right| {
        left["uri"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["uri"].as_str().unwrap_or_default())
    });
    Ok(resources)
}

fn resolve_resource_path(root: &Path, uri: &str) -> io::Result<PathBuf> {
    let relative = uri
        .strip_prefix("colmem://docs/")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsupported resource uri"))?;
    if relative.contains("..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resource uri must not contain path traversal",
        ));
    }
    Ok(docs_root(&root.display().to_string()).join(relative.replace('/', "\\")))
}

fn read_doc_resource(root: &Path, uri: &str) -> io::Result<Value> {
    let path = resolve_resource_path(root, uri)?;
    let text = fs::read_to_string(&path)?;
    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": detect_resource_mime_type(&path),
                "text": text
            }
        ]
    }))
}

fn tool_descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": output_schema,
    })
}

fn string_array_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" }
    })
}

fn fact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subject": { "type": "string" },
            "predicate": { "type": "string" },
            "object": { "type": "string" },
            "valid_from": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "null" }
                ]
            },
            "valid_to": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "null" }
                ]
            },
            "status": { "type": "string" },
            "reference_date": { "type": "string" },
            "confidence": { "type": "integer" },
            "evidence_ids": string_array_schema()
        },
        "required": ["subject", "predicate", "object", "confidence", "evidence_ids"],
        "additionalProperties": true
    })
}

fn fact_audit_event_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "timestamp": { "type": "string" },
            "action": { "type": "string" },
            "subject": { "type": "string" },
            "predicate": { "type": "string" },
            "object": { "type": "string" },
            "effective_at": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "null" }
                ]
            },
            "note": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "null" }
                ]
            }
        },
        "required": ["timestamp", "action", "subject", "predicate", "object"],
        "additionalProperties": true
    })
}

fn capability_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "kind": { "type": "string" },
            "provider": { "type": "string" },
            "version": { "type": "string" },
            "summary": { "type": "string" },
            "compatible_hosts": string_array_schema(),
            "compatible_roles": string_array_schema(),
            "project_tags": string_array_schema(),
            "permissions": string_array_schema(),
            "activation_hints": string_array_schema(),
            "stateful": { "type": "boolean" }
        },
        "required": ["id", "kind", "provider", "version", "summary", "stateful"],
        "additionalProperties": true
    })
}

fn agent_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "display_name": { "type": "string" },
            "role": { "type": "string" },
            "mission": { "type": "string" },
            "persona": {
                "type": "object",
                "properties": {
                    "voice": { "type": "string" },
                    "initiative": { "type": "integer" },
                    "risk_appetite": { "type": "integer" },
                    "explanation_depth": { "type": "integer" }
                },
                "required": ["voice", "initiative", "risk_appetite", "explanation_depth"],
                "additionalProperties": true
            },
            "habitat": {
                "type": "object",
                "properties": {
                    "home_space": { "type": "string" },
                    "accessible_spaces": string_array_schema(),
                    "watch_spaces": string_array_schema()
                },
                "required": ["home_space", "accessible_spaces", "watch_spaces"],
                "additionalProperties": true
            },
            "skill_profile": {
                "type": "object",
                "properties": {
                    "domains": {
                        "type": "object",
                        "additionalProperties": { "type": "integer" }
                    },
                    "preferred_capabilities": string_array_schema()
                },
                "required": ["domains", "preferred_capabilities"],
                "additionalProperties": true
            },
            "memory_priorities": {
                "type": "object",
                "additionalProperties": { "type": "integer" }
            },
            "manual_capability_modes": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        },
        "required": ["id", "display_name", "role", "mission", "persona", "habitat", "skill_profile"],
        "additionalProperties": true
    })
}

fn search_hit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chunk_id": { "type": "string" },
            "space_id": { "type": "string" },
            "space_path": string_array_schema(),
            "memory_path": { "type": "string" },
            "source_path": { "type": "string" },
            "line_start": { "type": "integer" },
            "line_end": { "type": "integer" },
            "ordinal": { "type": "integer" },
            "score": { "type": "integer" },
            "memory_path_match_count": { "type": "integer" },
            "snippet": { "type": "string" },
            "evidence_ids": string_array_schema(),
            "reasons": string_array_schema()
        },
        "required": ["chunk_id", "space_id", "source_path", "score", "snippet", "evidence_ids", "reasons"],
        "additionalProperties": true
    })
}

fn retrieval_plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "mode": { "type": "string" },
            "candidate_spaces": string_array_schema(),
            "enable_full_text": { "type": "boolean" },
            "enable_vectors": { "type": "boolean" },
            "enable_facts": { "type": "boolean" },
            "enable_rerank": { "type": "boolean" },
            "notes": string_array_schema()
        },
        "required": ["mode", "candidate_spaces", "enable_full_text", "enable_vectors", "enable_facts", "enable_rerank", "notes"],
        "additionalProperties": true
    })
}

fn context_pack_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agent_id": { "type": "string" },
            "project_id": { "type": "string" },
            "sections": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "entries": string_array_schema()
                    },
                    "required": ["title", "entries"],
                    "additionalProperties": true
                }
            },
            "memory_map": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "space_id": { "type": "string" },
                        "memory_path": { "type": "string" },
                        "evidence_count": { "type": "integer" },
                        "top_sources": string_array_schema()
                    },
                    "required": ["space_id", "memory_path", "evidence_count", "top_sources"],
                    "additionalProperties": true
                }
            },
            "citations": string_array_schema(),
            "policies": string_array_schema()
        },
        "required": ["agent_id", "project_id", "sections", "memory_map", "citations", "policies"],
        "additionalProperties": true
    })
}

fn memory_map_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "nodes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "label": { "type": "string" },
                        "parent_id": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "null" }
                            ]
                        },
                        "path": string_array_schema(),
                        "memory_path": { "type": "string" },
                        "tags": string_array_schema()
                    },
                    "required": ["id", "label", "path", "memory_path", "tags"],
                    "additionalProperties": true
                }
            },
            "links": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "from": { "type": "string" },
                        "to": { "type": "string" },
                        "kind": { "type": "string" },
                        "weight": { "type": "integer" }
                    },
                    "required": ["from", "to", "kind", "weight"],
                    "additionalProperties": true
                }
            }
        },
        "required": ["nodes", "links"],
        "additionalProperties": true
    })
}

fn capability_selection_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "enabled": {
                "type": "array",
                "items": capability_schema()
            },
            "disabled": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "audit": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "capability_id": { "type": "string" },
                        "outcome": { "type": "string", "enum": ["enabled", "disabled"] },
                        "binding_mode": { "type": "string", "enum": ["auto", "force_enabled", "force_disabled"] },
                        "project_required": { "type": "boolean" },
                        "task_requested": { "type": "boolean" },
                        "required_permissions": string_array_schema(),
                        "reasons": string_array_schema()
                    },
                    "required": ["capability_id", "outcome", "binding_mode", "project_required", "task_requested", "required_permissions", "reasons"],
                    "additionalProperties": true
                }
            }
        },
        "required": ["enabled", "disabled", "audit"],
        "additionalProperties": true
    })
}

fn evolution_preview_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "persona": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "voice_override": {
                                "oneOf": [
                                    { "type": "string" },
                                    { "type": "null" }
                                ]
                            },
                            "initiative_delta": { "type": "integer" },
                            "risk_delta": { "type": "integer" },
                            "explanation_delta": { "type": "integer" }
                        },
                        "additionalProperties": true
                    },
                    { "type": "null" }
                ]
            },
            "skill_deltas": {
                "type": "object",
                "additionalProperties": { "type": "integer" }
            },
            "preferred_capability_additions": string_array_schema(),
            "watch_space_additions": string_array_schema(),
            "memory_priority_deltas": {
                "type": "object",
                "additionalProperties": { "type": "integer" }
            }
        },
        "required": ["skill_deltas", "preferred_capability_additions", "watch_space_additions", "memory_priority_deltas"],
        "additionalProperties": true
    })
}

fn tools_payload() -> Value {
    json!({
        "tools": [
            tool_descriptor(
                "colmem_capability_list",
                "List registered capabilities.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "capabilities": {
                            "type": "array",
                            "items": capability_schema()
                        }
                    },
                    "required": ["capabilities"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_agent_inspect",
                "Inspect built-in agent profiles.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                json!({
                    "type": "array",
                    "items": agent_schema()
                })
            ),
            tool_descriptor(
                "colmem_query_plan",
                "Build a harness snapshot for a query.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "minLength": 1 },
                        "host": { "type": "string", "enum": ["claude_code", "codex", "cursor", "trae_ide", "openclaw", "generic_mcp"] },
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "reference_date": { "type": "string", "pattern": "^\\d{4}-\\d{2}-\\d{2}$" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "selected_agent": { "type": "string" },
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "fact_reference_date": { "type": "string" },
                        "fact_focus": { "type": "boolean" },
                        "hits": {
                            "type": "array",
                            "items": search_hit_schema()
                        },
                        "relevant_facts": {
                            "type": "array",
                            "items": fact_schema()
                        },
                        "retrieval_plan": retrieval_plan_schema(),
                        "context_pack": context_pack_schema(),
                        "selected_capabilities": capability_selection_schema(),
                        "evolution_preview": evolution_preview_schema()
                    },
                    "required": ["selected_agent", "retrieval_plan", "context_pack"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_fact_list",
                "List facts using active/history/scheduled/all filtering.",
                json!({
                    "type": "object",
                    "properties": {
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "reference_date": { "type": "string", "pattern": "^\\d{4}-\\d{2}-\\d{2}$" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "reference_date": { "type": "string" },
                        "facts": {
                            "type": "array",
                            "items": fact_schema()
                        }
                    },
                    "required": ["fact_scope", "reference_date", "facts"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_fact_query",
                "Query facts using active/history/scheduled/all filtering.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "minLength": 1 },
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "reference_date": { "type": "string", "pattern": "^\\d{4}-\\d{2}-\\d{2}$" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "null" }
                            ]
                        },
                        "fact_scope": { "type": "string", "enum": ["all", "active", "history", "scheduled"] },
                        "reference_date": { "type": "string" },
                        "facts": {
                            "type": "array",
                            "items": fact_schema()
                        }
                    },
                    "required": ["query", "fact_scope", "reference_date", "facts"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_fact_summary",
                "Return fact store counts for a reference date.",
                json!({
                    "type": "object",
                    "properties": {
                        "reference_date": { "type": "string", "pattern": "^\\d{4}-\\d{2}-\\d{2}$" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "reference_date": { "type": "string" },
                        "total": { "type": "integer" },
                        "active": { "type": "integer" },
                        "history": { "type": "integer" },
                        "scheduled": { "type": "integer" },
                        "inactive": { "type": "integer" },
                        "audit_events": { "type": "integer" }
                    },
                    "required": ["reference_date", "total", "active", "history", "scheduled", "inactive", "audit_events"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_fact_audit",
                "Inspect fact lifecycle audit events.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "minLength": 1 }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "null" }
                            ]
                        },
                        "events": {
                            "type": "array",
                            "items": fact_audit_event_schema()
                        }
                    },
                    "required": ["query", "events"],
                    "additionalProperties": true
                })
            ),
            tool_descriptor(
                "colmem_memory_map",
                "Return the structured workspace memory map.",
                json!({
                    "type": "object",
                    "properties": {
                        "space_id": { "type": "string", "minLength": 1 }
                    },
                    "additionalProperties": false
                }),
                memory_map_schema()
            ),
            tool_descriptor(
                "colmem_runtime_diagnostics",
                "Return runtime diagnostics.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "workspace": { "type": "string" },
                        "state_file": { "type": "string" },
                        "agents": { "type": "integer" },
                        "projects": { "type": "integer" },
                        "capabilities": { "type": "integer" },
                        "indexed_records": { "type": "integer" },
                        "indexed_chunks": { "type": "integer" },
                        "full_text_terms": { "type": "integer" },
                        "empty_evolution_patch": { "type": "string" }
                    },
                    "required": ["workspace", "state_file", "agents", "projects", "capabilities"],
                    "additionalProperties": true
                })
            )
        ]
    })
}

fn request_id(request: &Value) -> Option<Value> {
    request.get("id").cloned()
}

fn tool_name(request: &Value) -> Option<&str> {
    request
        .get("params")
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
        .or_else(|| request.get("name").and_then(Value::as_str))
}

fn argument_string<'a>(request: &'a Value, key: &str) -> Option<&'a str> {
    request
        .get("params")
        .and_then(|params| params.get("arguments"))
        .and_then(|args| args.get(key))
        .and_then(Value::as_str)
        .or_else(|| {
            request
                .get("params")
                .and_then(|params| params.get(key))
                .and_then(Value::as_str)
        })
        .or_else(|| request.get(key).and_then(Value::as_str))
}

fn required_argument_string<'a>(
    request: &'a Value,
    key: &str,
    tool_name: &str,
) -> Result<&'a str, String> {
    let Some(value) = argument_string(request, key) else {
        return Err(format!("{tool_name} requires a non-empty '{key}' argument"));
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{tool_name} requires a non-empty '{key}' argument"));
    }
    Ok(trimmed)
}

fn validate_iso_date(value: &str, key: &str, tool_name: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    let valid = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit());
    if valid {
        Ok(trimmed.to_string())
    } else {
        Err(format!("{tool_name} expects '{key}' in YYYY-MM-DD format"))
    }
}

fn parse_reference_date(request: &Value, tool_name: &str) -> Result<String, String> {
    match argument_string(request, "reference_date") {
        Some(value) => validate_iso_date(value, "reference_date", tool_name),
        None => Ok(InMemoryFactStore::today_iso_utc()),
    }
}

fn parse_fact_scope(request: &Value) -> Result<FactQueryScope, String> {
    match argument_string(request, "fact_scope").unwrap_or("all") {
        "all" => Ok(FactQueryScope::All),
        "active" => Ok(FactQueryScope::Active),
        "history" => Ok(FactQueryScope::History),
        "scheduled" => Ok(FactQueryScope::Scheduled),
        other => Err(format!("unknown fact_scope: {other}")),
    }
}

fn fact_scope_name(scope: FactQueryScope) -> &'static str {
    match scope {
        FactQueryScope::All => "all",
        FactQueryScope::Active => "active",
        FactQueryScope::History => "history",
        FactQueryScope::Scheduled => "scheduled",
    }
}

fn parse_host_id(request: &Value, tool_name: &str) -> Result<HostId, String> {
    match argument_string(request, "host") {
        Some(raw) => HostId::from_str(raw.trim())
            .map_err(|_| format!("{tool_name} received unsupported host '{raw}'")),
        None => Ok(HostId::GenericMcp),
    }
}

fn handle_request(
    request: &Value,
    default_root: &str,
    store: &WorkspaceStateStore,
) -> io::Result<Option<String>> {
    let id = request_id(request);
    let response_id = id.clone().unwrap_or(Value::Null);
    let is_notification = id.is_none();
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return if is_notification {
            Ok(None)
        } else {
            Ok(Some(error_response(
                response_id,
                -32600,
                "expected jsonrpc=\"2.0\"",
            )))
        };
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Ok(Some(error_response(response_id, -32600, "missing method")));
    };

    if matches!(
        method,
        "notifications/initialized"
            | "notifications/cancelled"
            | "notifications/progress"
            | "notifications/message"
            | "notifications/roots/list_changed"
            | "notifications/tools/list_changed"
    ) {
        return Ok(None);
    }

    if is_notification {
        return Ok(None);
    }

    let response = match method {
        "initialize" => result_response(
            response_id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "prompts": {
                        "listChanged": false
                    },
                    "resources": {
                        "subscribe": false,
                        "listChanged": false
                    },
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "colmem",
                    "version": "0.1.0"
                }
            }),
        ),
        "ping" => result_response(response_id, json!({})),
        "roots/list" => result_response(response_id, json!({ "roots": [] })),
        "logging/setLevel" => result_response(response_id, json!({})),
        "prompts/list" => result_response(response_id, json!({ "prompts": [] })),
        "resources/list" => result_response(
            response_id,
            json!({ "resources": list_doc_resources(Path::new(default_root))? }),
        ),
        "resources/read" => {
            let Some(uri) = argument_string(request, "uri") else {
                return Ok(Some(invalid_params_response(
                    response_id,
                    "resources/read requires a 'uri' argument",
                )));
            };
            match read_doc_resource(Path::new(default_root), uri.trim()) {
                Ok(result) => result_response(response_id, result),
                Err(err) if err.kind() == io::ErrorKind::InvalidInput => {
                    invalid_params_response(response_id, err.to_string())
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    error_response(response_id, -32002, &format!("resource not found: {err}"))
                }
                Err(err) => {
                    error_response(response_id, -32603, &format!("resource read error: {err}"))
                }
            }
        }
        "resources/templates/list" => {
            result_response(response_id, json!({ "resourceTemplates": [] }))
        }
        "tools/list" => result_response(response_id, tools_payload()),
        "tools/call" => {
            let state = match store.load_or_bootstrap() {
                Ok(state) => state,
                Err(err) => {
                    return Ok(Some(error_response(
                        response_id,
                        -32603,
                        &format!("workspace state error: {err}"),
                    )));
                }
            };
            let tool_name = tool_name(request).unwrap_or_default();
            if tool_name.is_empty() {
                return Ok(Some(invalid_params_response(
                    response_id,
                    "tools/call requires a 'name' field",
                )));
            }
            let payload = match tool_name {
                "colmem_capability_list" => json!({
                    "capabilities": serde_json::from_str::<Value>(&state.registry.to_json())
                        .map_err(io::Error::other)?
                }),
                "colmem_agent_inspect" => {
                    serde_json::to_value(&state.agents).map_err(io::Error::other)?
                }
                "colmem_query_plan" => {
                    let query = match required_argument_string(request, "query", tool_name) {
                        Ok(value) => value.to_string(),
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let fact_scope = match parse_fact_scope(request) {
                        Ok(scope) => scope,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let reference_date = match parse_reference_date(request, tool_name) {
                        Ok(value) => value,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let host_id = match parse_host_id(request, tool_name) {
                        Ok(value) => value,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let descriptor = HostDescriptor {
                        id: host_id,
                        display_name: "MCP Host",
                        transport: TransportKind::StdioMcp,
                        supports_stateful_plugins: true,
                        supported_capability_kinds: [
                            CapabilityKind::Skill,
                            CapabilityKind::Tool,
                            CapabilityKind::Plugin,
                            CapabilityKind::McpEndpoint,
                        ]
                        .into_iter()
                        .collect(),
                        install_hint: "Connect through stdio MCP.",
                    };
                    let agent = state
                        .agents
                        .iter()
                        .find(|candidate| candidate.id == "builder")
                        .cloned()
                        .unwrap();
                    let project = state.primary_project().cloned().unwrap();
                    let mut harness = standard_harness();
                    harness.registry = state.registry.clone();
                    harness.graph = state.spaces.clone();
                    harness.facts = state.facts.clone();
                    harness.index = state.index.clone();
                    let snapshot = harness.prepare_run_with_fact_scope(
                        &agent,
                        &project,
                        &HostContext::new(descriptor),
                        &TaskIntent {
                            kind: TaskKind::Query,
                            summary: query,
                            requested_capabilities: Default::default(),
                        },
                        fact_scope,
                        &reference_date,
                    );
                    serde_json::from_str::<Value>(&snapshot.to_json()).map_err(io::Error::other)?
                }
                "colmem_fact_list" => {
                    let fact_scope = match parse_fact_scope(request) {
                        Ok(scope) => scope,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let reference_date = match parse_reference_date(request, tool_name) {
                        Ok(value) => value,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    json!({
                        "fact_scope": fact_scope_name(fact_scope),
                        "reference_date": reference_date,
                        "facts": state
                            .facts
                            .facts_scoped(fact_scope, &reference_date)
                            .into_iter()
                            .map(|fact| {
                                serde_json::from_str::<Value>(&fact.to_json_with_status(&reference_date))
                                    .expect("fact json")
                            })
                            .collect::<Vec<_>>()
                    })
                }
                "colmem_fact_query" => {
                    let query = match required_argument_string(request, "query", tool_name) {
                        Ok(value) => value.to_string(),
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let fact_scope = match parse_fact_scope(request) {
                        Ok(scope) => scope,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    let reference_date = match parse_reference_date(request, tool_name) {
                        Ok(value) => value,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    json!({
                        "query": query,
                        "fact_scope": fact_scope_name(fact_scope),
                        "reference_date": reference_date,
                        "facts": state
                            .facts
                            .facts_for_query_scoped(&query, fact_scope, &reference_date)
                            .into_iter()
                            .map(|fact| {
                                serde_json::from_str::<Value>(&fact.to_json_with_status(&reference_date))
                                    .expect("fact json")
                            })
                            .collect::<Vec<_>>()
                    })
                }
                "colmem_fact_summary" => {
                    let reference_date = match parse_reference_date(request, tool_name) {
                        Ok(value) => value,
                        Err(message) => {
                            return Ok(Some(invalid_params_response(response_id, message)));
                        }
                    };
                    serde_json::from_str::<Value>(&state.facts.summary_json(&reference_date))
                        .map_err(io::Error::other)?
                }
                "colmem_fact_audit" => {
                    let query = match argument_string(request, "query") {
                        Some(value) => {
                            let trimmed = value.trim();
                            if trimmed.is_empty() {
                                return Ok(Some(invalid_params_response(
                                    response_id,
                                    format!(
                                        "{tool_name} requires 'query' to be non-empty when provided"
                                    ),
                                )));
                            }
                            Some(trimmed.to_string())
                        }
                        None => None,
                    };
                    let events = if let Some(query) = query.as_deref() {
                        state.facts.audit_events_for_query(query)
                    } else {
                        state.facts.audit_log().to_vec()
                    };
                    json!({
                        "query": query,
                        "events": events
                            .into_iter()
                            .map(|event| serde_json::from_str::<Value>(&event.to_json()).expect("audit json"))
                            .collect::<Vec<_>>()
                    })
                }
                "colmem_memory_map" => {
                    let memory_map = if let Some(space_id) = argument_string(request, "space_id") {
                        let space_id = space_id.trim();
                        if space_id.is_empty() {
                            return Ok(Some(invalid_params_response(
                                response_id,
                                "colmem_memory_map requires non-empty 'space_id' when provided",
                            )));
                        }
                        if !state.memory_paths.contains_key(space_id) {
                            return Ok(Some(invalid_params_response(
                                response_id,
                                format!("unknown space: {space_id}"),
                            )));
                        }
                        match state.spaces.to_memory_map_json_for_space(space_id) {
                            Some(memory_map) => memory_map,
                            None => {
                                return Ok(Some(invalid_params_response(
                                    response_id,
                                    format!("unknown space: {space_id}"),
                                )));
                            }
                        }
                    } else {
                        state.spaces.to_memory_map_json()
                    };
                    serde_json::from_str::<Value>(&memory_map).map_err(io::Error::other)?
                }
                "colmem_runtime_diagnostics" => {
                    let patch = EvolutionPatch::from_signal(&EvolutionSignal::default());
                    json!({
                        "workspace": default_root,
                        "state_file": store.paths.state_file.display().to_string(),
                        "agents": state.agents.len(),
                        "projects": state.projects.len(),
                        "capabilities": state.registry.capabilities.len(),
                        "indexed_records": state.index.records.len(),
                        "indexed_chunks": state.index.chunks.len(),
                        "full_text_terms": state.index.full_text.postings.len(),
                        "empty_evolution_patch": format!("{patch:?}"),
                    })
                }
                _ => return Ok(Some(error_response(response_id, -32601, "unknown tool"))),
            };
            tool_result_response(response_id, payload)
        }
        _ => error_response(response_id, -32601, "unknown method"),
    };

    Ok(Some(response))
}

pub fn handle_json_rpc_request(
    request_json: &str,
    default_root: &str,
) -> io::Result<Option<String>> {
    let request = serde_json::from_str::<Value>(request_json).map_err(io::Error::other)?;
    let store = WorkspaceStateStore::new(PathBuf::from(default_root));
    handle_request(&request, default_root, &store)
}

pub fn serve_stdio(default_root: &str) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let store = WorkspaceStateStore::new(PathBuf::from(default_root));
    let mut reader = stdin.lock();

    while let Some(message) = read_stdio_message(&mut reader)? {
        let request = match serde_json::from_str::<Value>(&message) {
            Ok(request) => request,
            Err(err) => {
                write_stdio_message(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("invalid json: {err}")),
                )?;
                continue;
            }
        };
        let Some(response) = handle_request(&request, default_root, &store)? else {
            continue;
        };

        write_stdio_message(&mut stdout, &response)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{Value, json};

    use super::{handle_request, read_stdio_message, write_stdio_message};
    use crate::storage::WorkspaceStateStore;

    fn temp_dir() -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!("colmem-mcp-test-{stamp}"));
        root
    }

    #[test]
    fn initialize_preserves_numeric_id() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "initialize"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle initialize")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!(42));
        assert_eq!(parsed["result"]["serverInfo"]["name"], json!("colmem"));
        assert_eq!(
            parsed["result"]["capabilities"]["tools"]["listChanged"],
            json!(false)
        );
        assert_eq!(
            parsed["result"]["capabilities"]["prompts"]["listChanged"],
            json!(false)
        );
        assert_eq!(
            parsed["result"]["capabilities"]["resources"]["subscribe"],
            json!(false)
        );
    }

    #[test]
    fn tools_list_returns_structured_result() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "abc",
            "method": "tools/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle tools/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("abc"));
        let tools = parsed["result"]["tools"].as_array().expect("tools array");
        assert!(parsed["result"]["content"].is_null());
        assert!(parsed["result"]["tools"][0]["inputSchema"].is_object());
        assert!(parsed["result"]["tools"][0]["outputSchema"].is_object());
        let tool_names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();
        assert_eq!(
            tool_names,
            vec![
                "colmem_capability_list",
                "colmem_agent_inspect",
                "colmem_query_plan",
                "colmem_fact_list",
                "colmem_fact_query",
                "colmem_fact_summary",
                "colmem_fact_audit",
                "colmem_memory_map",
                "colmem_runtime_diagnostics",
            ]
        );
        assert_eq!(
            parsed["result"]["tools"][0]["outputSchema"]["properties"]["capabilities"]["items"]["properties"]
                ["id"]["type"],
            json!("string")
        );
        assert_eq!(
            parsed["result"]["tools"][2]["outputSchema"]["properties"]["hits"]["items"]["properties"]
                ["score"]["type"],
            json!("integer")
        );
        assert_eq!(
            parsed["result"]["tools"][4]["outputSchema"]["properties"]["facts"]["items"]["properties"]
                ["subject"]["type"],
            json!("string")
        );
        let memory_map_tool = parsed["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .find(|tool| tool["name"] == json!("colmem_memory_map"))
            .expect("memory map tool");
        assert_eq!(
            memory_map_tool["outputSchema"]["properties"]["nodes"]["items"]["properties"]["memory_path"]
                ["type"],
            json!("string")
        );
    }

    #[test]
    fn capability_list_matches_declared_object_shape() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "caps",
            "method": "tools/call",
            "params": {
                "name": "colmem_capability_list",
                "arguments": {}
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle capability list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert!(parsed["result"]["structuredContent"]["capabilities"].is_array());
    }

    #[test]
    fn fact_query_tool_respects_scope_and_reference_date() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "facts",
            "method": "tools/call",
            "params": {
                "name": "colmem_fact_query",
                "arguments": {
                    "query": "colmem prefers retrieval",
                    "fact_scope": "active",
                    "reference_date": "2026-04-09"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle fact query")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");
        let fact_query = parsed["result"]["structuredContent"].clone();

        assert_eq!(fact_query["fact_scope"], json!("active"));
        assert_eq!(fact_query["reference_date"], json!("2026-04-09"));
        assert!(fact_query["facts"].is_array());
        assert_eq!(fact_query["facts"][0]["status"], json!("active"));
    }

    #[test]
    fn fact_summary_tool_reports_backend_counts() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "fact-summary",
            "method": "tools/call",
            "params": {
                "name": "colmem_fact_summary",
                "arguments": {
                    "reference_date": "2026-04-13"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle fact summary")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");
        let summary = parsed["result"]["structuredContent"].clone();

        assert_eq!(summary["reference_date"], json!("2026-04-13"));
        assert!(summary["total"].is_number());
        assert!(summary["audit_events"].is_number());
    }

    #[test]
    fn query_plan_structured_content_includes_fact_metadata() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "plan",
            "method": "tools/call",
            "params": {
                "name": "colmem_query_plan",
                "arguments": {
                    "query": "colmem prefers retrieval",
                    "fact_scope": "active",
                    "reference_date": "2026-04-09"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle query plan")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");
        let snapshot = parsed["result"]["structuredContent"].clone();

        assert_eq!(snapshot["fact_scope"], json!("active"));
        assert_eq!(snapshot["fact_reference_date"], json!("2026-04-09"));
        assert_eq!(snapshot["relevant_facts"][0]["status"], json!("active"));
    }

    #[test]
    fn memory_map_tool_returns_structured_space_paths() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "memory-map",
            "method": "tools/call",
            "params": {
                "name": "colmem_memory_map",
                "arguments": {}
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle memory map")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");
        let memory_map = parsed["result"]["structuredContent"].clone();

        assert!(memory_map["nodes"].is_array());
        assert!(memory_map["links"].is_array());
        assert!(
            memory_map["nodes"]
                .as_array()
                .expect("nodes")
                .iter()
                .any(|node| {
                    node["id"] == json!("retrieval")
                        && node["memory_path"] == json!("Workspace Root > Architecture > Retrieval")
                })
        );
    }

    #[test]
    fn memory_map_tool_can_filter_by_space_id() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "memory-map-filtered",
            "method": "tools/call",
            "params": {
                "name": "colmem_memory_map",
                "arguments": {
                    "space_id": "retrieval"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle filtered memory map")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");
        let nodes = parsed["result"]["structuredContent"]["nodes"]
            .as_array()
            .expect("nodes");

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["id"], json!("retrieval"));
        assert_eq!(
            nodes[0]["memory_path"],
            json!("Workspace Root > Architecture > Retrieval")
        );
    }

    #[test]
    fn query_plan_rejects_missing_query() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "bad-query",
            "method": "tools/call",
            "params": {
                "name": "colmem_query_plan",
                "arguments": {
                    "fact_scope": "active"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle invalid query plan")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32602));
        assert!(
            parsed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("requires a non-empty 'query' argument")
        );
    }

    #[test]
    fn fact_list_rejects_invalid_reference_date() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "bad-date",
            "method": "tools/call",
            "params": {
                "name": "colmem_fact_list",
                "arguments": {
                    "reference_date": "2026/04/09"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle invalid fact list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32602));
        assert!(
            parsed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("YYYY-MM-DD")
        );
    }

    #[test]
    fn query_plan_rejects_unknown_host() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "bad-host",
            "method": "tools/call",
            "params": {
                "name": "colmem_query_plan",
                "arguments": {
                    "query": "colmem prefers retrieval",
                    "host": "bad-host"
                }
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle invalid host")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32602));
        assert!(
            parsed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("unsupported host")
        );
    }

    #[test]
    fn tools_call_rejects_missing_name() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "missing-name",
            "method": "tools/call",
            "params": {
                "arguments": {}
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle invalid tools/call")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32602));
        assert!(
            parsed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("requires a 'name' field")
        );
    }

    #[test]
    fn missing_method_returns_invalid_request_error() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "missing-method"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle invalid request")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32600));
        assert_eq!(parsed["error"]["message"], json!("missing method"));
    }

    #[test]
    fn wrong_jsonrpc_version_is_rejected() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "1.0",
            "id": "wrong-version",
            "method": "ping"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle wrong jsonrpc")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32600));
        assert!(
            parsed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("jsonrpc")
        );
    }

    #[test]
    fn notification_methods_are_ignored() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {
                "requestId": "abc"
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle notification");

        assert!(response.is_none());
    }

    #[test]
    fn notifications_without_id_do_not_get_fabricated_response() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "colmem_runtime_diagnostics",
                "arguments": {}
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle notification-style request");

        assert!(response.is_none());
    }

    #[test]
    fn ping_returns_empty_result() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "ping-id",
            "method": "ping"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle ping")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("ping-id"));
        assert_eq!(parsed["result"], json!({}));
    }

    #[test]
    fn prompts_list_returns_empty_collection() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "prompts-id",
            "method": "prompts/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle prompts/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("prompts-id"));
        assert_eq!(parsed["result"]["prompts"], json!([]));
    }

    #[test]
    fn roots_list_returns_empty_collection() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "roots-id",
            "method": "roots/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle roots/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("roots-id"));
        assert_eq!(parsed["result"]["roots"], json!([]));
    }

    #[test]
    fn logging_set_level_acknowledges_request() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "log-level",
            "method": "logging/setLevel",
            "params": {
                "level": "warning"
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle logging/setLevel")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("log-level"));
        assert_eq!(parsed["result"], json!({}));
    }

    #[test]
    fn resources_list_returns_empty_collection() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "resources-id",
            "method": "resources/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle resources/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("resources-id"));
        assert_eq!(parsed["result"]["resources"], json!([]));
    }

    #[test]
    fn resource_templates_list_returns_empty_collection() {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("create temp dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "resource-templates-id",
            "method": "resources/templates/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle resources/templates/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["id"], json!("resource-templates-id"));
        assert_eq!(parsed["result"]["resourceTemplates"], json!([]));
    }

    #[test]
    fn resources_list_includes_docs_files() {
        let root = temp_dir();
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(root.join("docs").join("00-索引.md"), "# Docs").expect("write docs file");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "resources-with-docs",
            "method": "resources/list"
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle resources/list")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(
            parsed["result"]["resources"][0]["uri"],
            json!("colmem://docs/00-索引.md")
        );
    }

    #[test]
    fn resources_read_returns_doc_text() {
        let root = temp_dir();
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        fs::write(root.join("docs").join("00-索引.md"), "# Docs").expect("write docs file");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "resource-read",
            "method": "resources/read",
            "params": {
                "uri": "colmem://docs/00-索引.md"
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle resources/read")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(
            parsed["result"]["contents"][0]["mimeType"],
            json!("text/markdown")
        );
        assert_eq!(parsed["result"]["contents"][0]["text"], json!("# Docs"));
    }

    #[test]
    fn resources_read_rejects_path_traversal() {
        let root = temp_dir();
        fs::create_dir_all(root.join("docs")).expect("create docs dir");
        let store = WorkspaceStateStore::new(&root);
        store.load_or_bootstrap().expect("bootstrap state");
        let request = json!({
            "jsonrpc": "2.0",
            "id": "resource-bad",
            "method": "resources/read",
            "params": {
                "uri": "colmem://docs/../secret.txt"
            }
        });

        let response = handle_request(&request, &root.display().to_string(), &store)
            .expect("handle resources/read")
            .expect("response");
        let parsed: Value = serde_json::from_str(&response).expect("parse response");

        assert_eq!(parsed["error"]["code"], json!(-32602));
    }

    #[test]
    fn framed_stdio_messages_round_trip() {
        let request = "{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"ping\"}";
        let mut reader = Cursor::new(format!(
            "Content-Length: {}\r\n\r\n{}",
            request.len(),
            request
        ));
        let message = read_stdio_message(&mut reader)
            .expect("read message")
            .expect("some message");
        assert_eq!(message, request);

        let mut output = Vec::new();
        write_stdio_message(
            &mut output,
            "{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"result\":{}}",
        )
        .expect("write message");
        let output_text = String::from_utf8(output).expect("utf8 output");
        assert!(output_text.starts_with("Content-Length: "));
        assert!(output_text.contains("\r\n\r\n{\"jsonrpc\":\"2.0\""));
    }
}
