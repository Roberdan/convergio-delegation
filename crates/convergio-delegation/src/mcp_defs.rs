//! MCP tool definitions for the delegation extension.

use convergio_types::extension::McpToolDef;
use serde_json::json;

pub fn delegation_tools() -> Vec<McpToolDef> {
    vec![
        McpToolDef {
            name: "cvg_mesh_delegate".into(),
            description: "Delegate a task to a mesh peer.".into(),
            method: "POST".into(),
            path: "/api/mesh/delegate".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "peer_id": {"type": "string"},
                    "task": {"type": "string"}
                },
                "required": ["peer_id", "task"]
            }),
            min_ring: "trusted".into(),
            path_params: vec![],
        },
        McpToolDef {
            name: "cvg_delegate_spawn".into(),
            description: "Spawn a delegated agent on a peer.".into(),
            method: "POST".into(),
            path: "/api/delegate/spawn".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "peer": {"type": "string", "description": "Peer name from peers.conf (e.g. macProM1, omarchy)"},
                    "plan_id": {"type": "integer", "description": "Plan ID to delegate"}
                },
                "required": ["peer", "plan_id"]
            }),
            min_ring: "trusted".into(),
            path_params: vec![],
        },
        McpToolDef {
            name: "cvg_delegation_status".into(),
            description: "Get status of a delegation.".into(),
            method: "GET".into(),
            path: "/api/delegate/status/:delegation_id".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "delegation_id": {"type": "string"}
                },
                "required": ["delegation_id"]
            }),
            min_ring: "community".into(),
            path_params: vec!["delegation_id".into()],
        },
        McpToolDef {
            name: "cvg_delegation_list".into(),
            description: "List all delegations.".into(),
            method: "GET".into(),
            path: "/api/delegate/list".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            min_ring: "community".into(),
            path_params: vec![],
        },
    ]
}
