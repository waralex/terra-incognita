//! Data schema: transaction metadata fields and managed types.
//!
//! Parsed from a YAML file that the agent developer provides.
//! Defines what fields accompany each transaction and what managed types
//! (like tasks) exist with their fields and optional lifecycle.
//!
//! # Example
//!
//! ```yaml
//! transaction_meta:
//!   reasoning:
//!     type: text
//!     required: true
//!   question:
//!     type: text
//!   answer:
//!     type: text
//!
//! managed_types:
//!   task:
//!     fields:
//!       goal: { type: json, required: true }
//!       reasoning: { type: text, required: true }
//!       context: { type: json }
//!       notes: { type: json }
//!       resolution: { type: json }
//!     lifecycle:
//!       states: [open, closed]
//!       initial: open
//!       terminal: [closed]
//!       transitions:
//!         open: [closed]
//! ```

use std::collections::BTreeSet;

use indexmap::IndexMap;
use std::path::Path;

use serde::Deserialize;

/// Data schema: what fields transactions carry and what managed types exist.
#[derive(Debug, Clone, Deserialize)]
pub struct DataSchema {
    /// Fields that accompany every transaction (e.g. reasoning, question, answer).
    #[serde(default)]
    pub transaction_meta: IndexMap<String, FieldDef>,

    /// Managed types — versioned record types with optional lifecycle.
    #[serde(default)]
    pub managed_types: IndexMap<String, ManagedTypeDef>,
}

/// Definition of a single field in transaction metadata or a managed type.
#[derive(Debug, Clone, Deserialize)]
pub struct FieldDef {
    /// Value type hint. "text" = string, "json" = arbitrary JSON.
    /// Not enforced at storage level — purely for prompt generation and docs.
    #[serde(rename = "type", default = "default_field_type")]
    pub field_type: FieldType,

    /// Whether the field must be present.
    #[serde(default)]
    pub required: bool,
}

/// Field type hint — how the value should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    Json,
}

fn default_field_type() -> FieldType {
    FieldType::Json
}

/// Definition of a managed type (e.g. task, investigation, review).
#[derive(Debug, Clone, Deserialize)]
pub struct ManagedTypeDef {
    /// Fields on each record of this type.
    pub fields: IndexMap<String, FieldDef>,

    /// Optional lifecycle — state machine for the record.
    /// If absent, records are just versioned key-value bags with no status.
    #[serde(default)]
    pub lifecycle: Option<LifecycleDef>,
}

/// State machine definition for a managed type.
#[derive(Debug, Clone, Deserialize)]
pub struct LifecycleDef {
    /// All possible states.
    pub states: Vec<String>,

    /// Initial state assigned on creation.
    pub initial: String,

    /// Terminal states — excluded from default listings (not loaded into agent context).
    #[serde(default)]
    pub terminal: Vec<String>,

    /// Allowed transitions: state → [reachable states].
    /// A state not present as a key has no outgoing transitions (sink).
    #[serde(default)]
    pub transitions: IndexMap<String, Vec<String>>,
}

/// Errors from config parsing and validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("managed type \"{type_name}\": {message}")]
    Lifecycle {
        type_name: String,
        message: String,
    },

    #[error("managed type \"{type_name}\": field \"{field}\" conflicts with reserved lifecycle field \"state\"")]
    ReservedField {
        type_name: String,
        field: String,
    },
}

impl DataSchema {
    /// Parse config from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_yaml::from_str(yaml)?;
        config.validate()?;
        Ok(config)
    }

    /// Parse config from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Validate internal consistency after deserialization.
    fn validate(&self) -> Result<(), ConfigError> {
        for (name, def) in &self.managed_types {
            // "state" is reserved when lifecycle is present.
            if def.lifecycle.is_some() && def.fields.contains_key("state") {
                return Err(ConfigError::ReservedField {
                    type_name: name.clone(),
                    field: "state".into(),
                });
            }

            if let Some(lc) = &def.lifecycle {
                let known: BTreeSet<&str> = lc.states.iter().map(|s| s.as_str()).collect();

                if known.is_empty() {
                    return Err(ConfigError::Lifecycle {
                        type_name: name.clone(),
                        message: "states list is empty".into(),
                    });
                }

                if !known.contains(lc.initial.as_str()) {
                    return Err(ConfigError::Lifecycle {
                        type_name: name.clone(),
                        message: format!(
                            "initial state \"{}\" is not in states list",
                            lc.initial
                        ),
                    });
                }

                for t in &lc.terminal {
                    if !known.contains(t.as_str()) {
                        return Err(ConfigError::Lifecycle {
                            type_name: name.clone(),
                            message: format!(
                                "terminal state \"{}\" is not in states list",
                                t
                            ),
                        });
                    }
                }

                for (from, targets) in &lc.transitions {
                    if !known.contains(from.as_str()) {
                        return Err(ConfigError::Lifecycle {
                            type_name: name.clone(),
                            message: format!(
                                "transition source \"{}\" is not in states list",
                                from
                            ),
                        });
                    }
                    for to in targets {
                        if !known.contains(to.as_str()) {
                            return Err(ConfigError::Lifecycle {
                                type_name: name.clone(),
                                message: format!(
                                    "transition target \"{}\" (from \"{}\") is not in states list",
                                    to, from
                                ),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let yaml = r#"
transaction_meta:
  reasoning:
    type: text
    required: true
  question:
    type: text
  answer:
    type: text

managed_types:
  task:
    fields:
      goal: { type: json, required: true }
      reasoning: { type: text, required: true }
      context: { type: json }
      notes: { type: json }
      resolution: { type: json }
    lifecycle:
      states: [open, closed]
      initial: open
      terminal: [closed]
      transitions:
        open: [closed]
"#;
        let config = DataSchema::from_yaml(yaml).unwrap();

        assert_eq!(config.transaction_meta.len(), 3);
        assert!(config.transaction_meta["reasoning"].required);
        assert!(!config.transaction_meta["question"].required);
        assert_eq!(config.transaction_meta["reasoning"].field_type, FieldType::Text);

        let task = &config.managed_types["task"];
        assert_eq!(task.fields.len(), 5);
        assert!(task.fields["goal"].required);
        assert!(!task.fields["notes"].required);

        let lc = task.lifecycle.as_ref().unwrap();
        assert_eq!(lc.states, vec!["open", "closed"]);
        assert_eq!(lc.initial, "open");
        assert_eq!(lc.terminal, vec!["closed"]);
        assert_eq!(lc.transitions["open"], vec!["closed"]);
        assert!(!lc.transitions.contains_key("closed"));
    }

    #[test]
    fn empty_config_is_valid() {
        let yaml = "{}";
        let config = DataSchema::from_yaml(yaml).unwrap();
        assert!(config.transaction_meta.is_empty());
        assert!(config.managed_types.is_empty());
    }

    #[test]
    fn managed_type_without_lifecycle() {
        let yaml = r#"
managed_types:
  note:
    fields:
      content: { type: text, required: true }
      tags: { type: json }
"#;
        let config = DataSchema::from_yaml(yaml).unwrap();
        let note = &config.managed_types["note"];
        assert!(note.lifecycle.is_none());
        assert_eq!(note.fields.len(), 2);
    }

    #[test]
    fn multi_state_lifecycle() {
        let yaml = r#"
managed_types:
  review:
    fields:
      summary: { type: text, required: true }
    lifecycle:
      states: [draft, in_review, approved, rejected]
      initial: draft
      terminal: [approved, rejected]
      transitions:
        draft: [in_review]
        in_review: [approved, rejected, draft]
"#;
        let config = DataSchema::from_yaml(yaml).unwrap();
        let lc = config.managed_types["review"].lifecycle.as_ref().unwrap();
        assert_eq!(lc.initial, "draft");
        assert_eq!(lc.terminal, vec!["approved", "rejected"]);
        assert_eq!(lc.transitions["in_review"], vec!["approved", "rejected", "draft"]);
    }

    #[test]
    fn initial_state_not_in_states() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      x: { type: text }
    lifecycle:
      states: [a, b]
      initial: c
      transitions: {}
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("initial state \"c\" is not in states list"));
    }

    #[test]
    fn terminal_state_not_in_states() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      x: { type: text }
    lifecycle:
      states: [a, b]
      initial: a
      terminal: [z]
      transitions: {}
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("terminal state \"z\" is not in states list"));
    }

    #[test]
    fn transition_source_not_in_states() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      x: { type: text }
    lifecycle:
      states: [a, b]
      initial: a
      transitions:
        ghost: [b]
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("transition source \"ghost\" is not in states list"));
    }

    #[test]
    fn transition_target_not_in_states() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      x: { type: text }
    lifecycle:
      states: [a, b]
      initial: a
      transitions:
        a: [nowhere]
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("transition target \"nowhere\""));
    }

    #[test]
    fn empty_states_list() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      x: { type: text }
    lifecycle:
      states: []
      initial: a
      transitions: {}
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("states list is empty"));
    }

    #[test]
    fn reserved_field_state_with_lifecycle() {
        let yaml = r#"
managed_types:
  bad:
    fields:
      state: { type: text }
    lifecycle:
      states: [open]
      initial: open
"#;
        let err = DataSchema::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("reserved lifecycle field"));
    }

    #[test]
    fn field_state_ok_without_lifecycle() {
        let yaml = r#"
managed_types:
  ok:
    fields:
      state: { type: text }
"#;
        let config = DataSchema::from_yaml(yaml).unwrap();
        assert!(config.managed_types["ok"].fields.contains_key("state"));
    }

    #[test]
    fn default_field_type_is_json() {
        let yaml = r#"
transaction_meta:
  data:
    required: true
"#;
        let config = DataSchema::from_yaml(yaml).unwrap();
        assert_eq!(config.transaction_meta["data"].field_type, FieldType::Json);
    }
}
