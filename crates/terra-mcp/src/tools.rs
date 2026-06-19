//! The terra memory toolset — intent-shaped MCP tools over `POST /query`.
//!
//! Each tool is defined once here: its name + description + input schema
//! (what the model sees in `tools/list`) and its mapping to a terra command
//! body (`build_query`). The descriptions are the tool's prompt: they encode
//! the memory policy (provenance, status defaults, slug conventions) so the
//! caller does not have to.

use serde_json::{json, Map, Value};

/// Initial lifecycle state for each managed `note` kind, per the memory schema.
fn initial_state(kind: &str) -> Option<&'static str> {
    match kind {
        "task" | "open_question" => Some("open"),
        "convention" => Some("active"),
        "decision" => Some("accepted"),
        _ => None,
    }
}

/// Fields allowed on each managed `note` kind, per the memory schema.
fn allowed_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "task" => &["content", "scope", "horizon", "rationale"],
        "convention" => &["content", "scope", "rationale"],
        "decision" => &["content", "rationale", "scope"],
        "open_question" => &["content", "scope"],
        _ => &[],
    }
}

/// The tool definitions advertised in `tools/list`.
pub fn list() -> Vec<Value> {
    vec![
        json!({
            "name": "recall",
            "description": "Recall what is known about an entity by its slug: its properties with provenance (source), epistemic status (fact/observation/hypothesis), and when each was asserted. Use before answering or writing, to see what is already known.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity slug, e.g. cube or cube.cubestore" }
                },
                "required": ["entity"]
            }
        }),
        json!({
            "name": "grep",
            "description": "Deterministic regex search over entities. Use to scope a project (slug ^cube\\.) or find by a property value. Matches against the chosen fields; a match in any includes the whole entity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Rust regex (case-sensitive; prefix (?i) to ignore case)" },
                    "in": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["slug", "property", "value", "reasoning"] },
                        "description": "Fields to match against; default [slug]"
                    },
                    "limit": { "type": "integer", "description": "Max entities, default 50" }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "remember",
            "description": "Record facts about an entity. Upsert: creates the entity if its slug is new (description then required), updates it otherwise. Store only what is not already in code/git: who/why/decisions/preferences. source = where it came from (user | code:<path> | slack:<ch> | inference). status: fact (confirmed/consolidated) | observation (default, seen it) | hypothesis (guess).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string", "description": "Entity slug (dotted namespace, e.g. cube.cubestore)" },
                    "facts": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "property": { "type": "string" },
                                "value": {}
                            },
                            "required": ["property", "value"]
                        },
                        "description": "Property assertions to record"
                    },
                    "reasoning": { "type": "string", "description": "Why this is believed / where learned" },
                    "description": { "type": "string", "description": "One-line what-it-is; required when the entity is new" },
                    "source": { "type": "string", "description": "Provenance: user | code:<path> | slack:<ch> | inference" },
                    "status": { "type": "string", "enum": ["fact", "observation", "hypothesis"] }
                },
                "required": ["entity", "facts", "reasoning"]
            }
        }),
        json!({
            "name": "link",
            "description": "Record a relation between entities as a property whose value is another entity's slug (e.g. part_of, depends_on, owned_by). The 'from' entity must already exist.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "relation": { "type": "string", "description": "Relation property, e.g. depends_on" },
                    "to": { "type": "string", "description": "Target entity slug" },
                    "reasoning": { "type": "string" },
                    "source": { "type": "string" }
                },
                "required": ["from", "relation", "to", "reasoning"]
            }
        }),
        json!({
            "name": "note",
            "description": "Create a managed record. kind: task (something to do; horizon next|someday), open_question (something to learn), decision (a choice + why), convention (a standing preference). Pick a short kebab slug — it is internal addressing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["task", "open_question", "decision", "convention"] },
                    "slug": { "type": "string", "description": "Short kebab id, e.g. design-mcp-tools" },
                    "content": { "type": "string" },
                    "reasoning": { "type": "string" },
                    "scope": { "type": "string", "description": "Project/area, e.g. cube" },
                    "rationale": { "type": "string" },
                    "horizon": { "type": "string", "enum": ["next", "someday"], "description": "task only: commitment level" }
                },
                "required": ["kind", "slug", "content", "reasoning"]
            }
        }),
        json!({
            "name": "update_note",
            "description": "Update a managed record: transition its state (e.g. task open->done, open_question open->answered, convention active->retired) and/or change fields. Append-only: prior versions stay.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["task", "open_question", "decision", "convention"] },
                    "slug": { "type": "string" },
                    "reasoning": { "type": "string" },
                    "state": { "type": "string" },
                    "content": { "type": "string" },
                    "scope": { "type": "string" },
                    "rationale": { "type": "string" },
                    "horizon": { "type": "string", "enum": ["next", "someday"] }
                },
                "required": ["kind", "slug", "reasoning"]
            }
        }),
        json!({
            "name": "list",
            "description": "List visible managed records of a kind (task/open_question/decision/convention). Use at the start of work to load open tasks and questions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["task", "open_question", "decision", "convention"] }
                },
                "required": ["kind"]
            }
        }),
        json!({
            "name": "history",
            "description": "How knowledge about an entity evolved: a snapshot at every transaction that touched it, newest first, with what changed and why. Use to see provenance over time or whether a fact is stale.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string" },
                    "property": { "type": "string", "description": "Only transactions that touched this property" },
                    "limit": { "type": "integer", "description": "Max entries, default 50" }
                },
                "required": ["entity"]
            }
        }),
        json!({
            "name": "touch",
            "description": "Mark an entity as relevant to current work without changing it. A lightweight signal of context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string" },
                    "reasoning": { "type": "string" }
                },
                "required": ["entity", "reasoning"]
            }
        }),
        json!({
            "name": "retract",
            "description": "Record that an entity no longer holds (soft-delete with reasoning). Append-only: prior assertions stay queryable; the entity may be re-created later.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string" },
                    "reasoning": { "type": "string" }
                },
                "required": ["entity", "reasoning"]
            }
        }),
    ]
}

/// Map a tool call (`name` + `arguments`) to a terra command body.
pub fn build_query(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "recall" => Ok(json!({ "command": "entity.get", "entity": req_str(args, "entity")? })),

        "grep" => {
            let mut q = json!({ "command": "entities.grep", "pattern": req_str(args, "pattern")? });
            copy_into(&mut q, args, &["in", "limit"]);
            Ok(q)
        }

        "remember" => {
            let mut entity_meta = json!({ "reasoning": req_str(args, "reasoning")? });
            if let Some(src) = opt_str(args, "source") {
                entity_meta["source"] = json!(src);
            }
            let mut entity = json!({
                "slug": req_str(args, "entity")?,
                "meta": entity_meta,
                "properties": req(args, "facts")?,
            });
            if let Some(d) = opt(args, "description") {
                entity["description"] = d.clone();
            }
            if let Some(s) = opt_str(args, "status") {
                entity["status"] = json!(s);
            }
            Ok(json!({
                "command": "transaction",
                "meta": { "reasoning": req_str(args, "reasoning")? },
                "write": [entity],
            }))
        }

        "link" => {
            let mut entity_meta = json!({ "reasoning": req_str(args, "reasoning")? });
            if let Some(src) = opt_str(args, "source") {
                entity_meta["source"] = json!(src);
            }
            Ok(json!({
                "command": "transaction",
                "meta": { "reasoning": req_str(args, "reasoning")? },
                "write": [{
                    "slug": req_str(args, "from")?,
                    "meta": entity_meta,
                    "properties": [{
                        "property": req_str(args, "relation")?,
                        "value": req_str(args, "to")?,
                    }],
                }],
            }))
        }

        "note" => {
            let kind = req_str(args, "kind")?;
            let state = initial_state(&kind).ok_or_else(|| format!("unknown kind: {kind}"))?;
            Ok(json!({
                "command": "transaction",
                "meta": { "reasoning": req_str(args, "reasoning")? },
                "create_managed": [{
                    "type_name": kind,
                    "slug": req_str(args, "slug")?,
                    "state": state,
                    "fields": pick(args, allowed_fields(&kind)),
                }],
            }))
        }

        "update_note" => {
            let kind = req_str(args, "kind")?;
            let mut item = json!({
                "type_name": kind,
                "slug": req_str(args, "slug")?,
                "fields": pick(args, allowed_fields(&kind)),
            });
            if let Some(s) = opt_str(args, "state") {
                item["state"] = json!(s);
            }
            Ok(json!({
                "command": "transaction",
                "meta": { "reasoning": req_str(args, "reasoning")? },
                "update_managed": [item],
            }))
        }

        "list" => Ok(json!({ "command": "managed.list", "type_name": req_str(args, "kind")? })),

        "history" => {
            let mut q = json!({ "command": "entity.history", "entity": req_str(args, "entity")? });
            copy_into(&mut q, args, &["property", "limit"]);
            Ok(q)
        }

        "touch" => Ok(json!({
            "command": "transaction",
            "meta": { "reasoning": req_str(args, "reasoning")? },
            "touch": [{ "entity": req_str(args, "entity")?, "reasoning": req_str(args, "reasoning")? }],
        })),

        "retract" => Ok(json!({
            "command": "transaction",
            "meta": { "reasoning": req_str(args, "reasoning")? },
            "delete": [{ "entity": req_str(args, "entity")?, "reasoning": req_str(args, "reasoning")? }],
        })),

        other => Err(format!("unknown tool: {other}")),
    }
}

/// Read a required argument as raw JSON.
fn req<'a>(args: &'a Value, key: &str) -> Result<&'a Value, String> {
    args.get(key)
        .filter(|v| !v.is_null())
        .ok_or_else(|| format!("missing required argument: {key}"))
}

/// Read a required string argument.
fn req_str(args: &Value, key: &str) -> Result<String, String> {
    req(args, key)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("argument {key} must be a string"))
}

/// Read an optional argument as raw JSON (absent or null → None).
fn opt<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|v| !v.is_null())
}

/// Read an optional string argument.
fn opt_str(args: &Value, key: &str) -> Option<String> {
    opt(args, key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Copy the given keys from `args` into a target object when present.
fn copy_into(target: &mut Value, args: &Value, keys: &[&str]) {
    for key in keys {
        if let Some(v) = opt(args, key) {
            target[*key] = v.clone();
        }
    }
}

/// Build a fields object from the allowed keys present in `args`.
fn pick(args: &Value, keys: &[&str]) -> Value {
    let mut map = Map::new();
    for key in keys {
        if let Some(v) = opt(args, key) {
            map.insert((*key).to_string(), v.clone());
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_maps_to_entity_get() {
        let q = build_query("recall", &json!({ "entity": "cube" })).unwrap();
        assert_eq!(q["command"], "entity.get");
        assert_eq!(q["entity"], "cube");
    }

    #[test]
    fn remember_puts_source_on_entity_meta_and_reasoning_on_both() {
        let q = build_query(
            "remember",
            &json!({
                "entity": "terra",
                "description": "store",
                "reasoning": "user told me",
                "source": "user",
                "status": "fact",
                "facts": [{ "property": "language", "value": "Rust" }]
            }),
        )
        .unwrap();
        assert_eq!(q["command"], "transaction");
        assert_eq!(q["meta"]["reasoning"], "user told me");
        let e = &q["write"][0];
        assert_eq!(e["slug"], "terra");
        assert_eq!(e["description"], "store");
        assert_eq!(e["status"], "fact");
        assert_eq!(e["meta"]["reasoning"], "user told me");
        assert_eq!(e["meta"]["source"], "user");
        assert_eq!(e["properties"][0]["property"], "language");
    }

    #[test]
    fn note_uses_initial_state_and_drops_foreign_fields() {
        // `horizon` is a task-only field; it must not leak onto a convention.
        let q = build_query(
            "note",
            &json!({
                "kind": "convention",
                "slug": "minimal-comments",
                "content": "prefer minimal comments",
                "horizon": "next",
                "reasoning": "user preference"
            }),
        )
        .unwrap();
        let m = &q["create_managed"][0];
        assert_eq!(m["type_name"], "convention");
        assert_eq!(m["state"], "active");
        assert_eq!(m["fields"]["content"], "prefer minimal comments");
        assert!(m["fields"].get("horizon").is_none());
    }

    #[test]
    fn link_writes_relation_as_slug_value() {
        let q = build_query(
            "link",
            &json!({ "from": "cube", "relation": "depends_on", "to": "cubestore", "reasoning": "uses it" }),
        )
        .unwrap();
        let p = &q["write"][0]["properties"][0];
        assert_eq!(p["property"], "depends_on");
        assert_eq!(p["value"], "cubestore");
    }

    #[test]
    fn missing_required_argument_errors() {
        let err = build_query("recall", &json!({})).unwrap_err();
        assert!(err.contains("entity"));
    }

    #[test]
    fn unknown_tool_errors() {
        assert!(build_query("nope", &json!({})).is_err());
    }
}
