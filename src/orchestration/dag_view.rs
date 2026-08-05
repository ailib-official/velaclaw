//! Read-only DAG view-model for ORCH-DAG-VIS-001 (fixtures / candidates).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DagNodeView {
    pub id: String,
    pub task_type: Option<String>,
    pub capabilities: Vec<String>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DagGraphView {
    pub id: String,
    pub entry: String,
    pub schema_version: Option<String>,
    pub nodes: Vec<DagNodeView>,
    pub edges: Vec<(String, String)>,
    pub valid_shape: bool,
    pub notes: Vec<String>,
}

/// Build a visualization view from a DAG JSON value (L2 template or L4 candidate).
#[must_use]
pub fn graph_from_value(v: &Value) -> DagGraphView {
    let mut notes = Vec::new();
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_string();
    let entry = v
        .get("entry")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let schema_version = v
        .get("schema_version")
        .and_then(|x| x.as_str())
        .map(str::to_string);

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let Some(arr) = v.get("nodes").and_then(|n| n.as_array()) else {
        notes.push("missing nodes array".into());
        return DagGraphView {
            id,
            entry,
            schema_version,
            nodes,
            edges,
            valid_shape: false,
            notes,
        };
    };

    for n in arr {
        let nid = n
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if nid.is_empty() {
            notes.push("node missing id".into());
            continue;
        }
        let task_type = n
            .get("task_type")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let capabilities = n
            .pointer("/model_selector/capabilities")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let next = match n.get("next") {
            Some(Value::Null) | None => None,
            Some(x) => x.as_str().map(str::to_string),
        };
        if let Some(ref nxt) = next {
            edges.push((nid.clone(), nxt.clone()));
        }
        nodes.push(DagNodeView {
            id: nid,
            task_type,
            capabilities,
            next,
        });
    }

    let valid_shape = !entry.is_empty() && !nodes.is_empty();
    if entry.is_empty() {
        notes.push("missing entry".into());
    }
    DagGraphView {
        id,
        entry,
        schema_version,
        nodes,
        edges,
        valid_shape,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn linear_template_edges() {
        let v = json!({
            "schema_version": "0.1.0",
            "id": "review-template",
            "entry": "a",
            "nodes": [
                {
                    "id": "a",
                    "task_type": "review",
                    "model_selector": { "capabilities": ["coding"] },
                    "next": "b"
                },
                {
                    "id": "b",
                    "task_type": "summarize",
                    "model_selector": { "capabilities": ["speed"] },
                    "next": null
                }
            ]
        });
        let g = graph_from_value(&v);
        assert!(g.valid_shape);
        assert_eq!(g.edges, vec![("a".into(), "b".into())]);
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].capabilities, vec!["coding".to_string()]);
    }
}
