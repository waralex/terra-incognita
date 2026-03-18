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
//!       initial: open
//!       visible: [open]
//! ```

use indexmap::IndexMap;
use std::path::Path;

use serde::Deserialize;

/// Data schema: what fields transactions carry and what managed types exist.
#[derive(Debug, Clone, Deserialize)]
pub struct DataSchema {
    /// Fields that accompany every transaction (e.g. reasoning, question, answer).
    #[serde(default)]
    pub transaction_meta: IndexMap<String, FieldDef>,

    /// Fields that accompany each entity change (batch of assertions).
    #[serde(default)]
    pub entity_change_meta: IndexMap<String, FieldDef>,

    /// Fields that describe a branch (e.g. reasoning, purpose).
    #[serde(default)]
    pub branch_meta: IndexMap<String, FieldDef>,

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

/// Lifecycle definition for a managed type.
///
/// The lifecycle defines valid states, which state is assigned on creation,
/// and which states are included in default listings.
#[derive(Debug, Clone, Deserialize)]
pub struct LifecycleDef {
    /// State assigned on creation.
    pub initial: String,

    /// Full set of valid states. If empty in YAML, derived from `{initial} ∪ visible`
    /// during validation.
    #[serde(default)]
    pub states: Vec<String>,

    /// States included in default listings (loaded into agent context).
    /// If empty, all states are visible by default.
    #[serde(default)]
    pub visible: Vec<String>,
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
        let mut config: Self = serde_yaml::from_str(yaml)?;
        config.validate()?;
        Ok(config)
    }

    /// Parse config from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Validate internal consistency after deserialization.
    /// Populates `states` from `{initial} ∪ visible` when not explicitly provided.
    fn validate(&mut self) -> Result<(), ConfigError> {
        for (name, def) in &mut self.managed_types {
            if def.lifecycle.is_some() && def.fields.contains_key("state") {
                return Err(ConfigError::ReservedField {
                    type_name: name.clone(),
                    field: "state".into(),
                });
            }

            if let Some(lc) = &mut def.lifecycle {
                if lc.initial.is_empty() {
                    return Err(ConfigError::Lifecycle {
                        type_name: name.clone(),
                        message: "initial state is empty".into(),
                    });
                }

                for t in &lc.visible {
                    if t.is_empty() {
                        return Err(ConfigError::Lifecycle {
                            type_name: name.clone(),
                            message: "visible state is empty".into(),
                        });
                    }
                }

                // Derive states from {initial} ∪ visible when not provided.
                if lc.states.is_empty() {
                    let mut derived = std::collections::BTreeSet::new();
                    derived.insert(lc.initial.clone());
                    derived.extend(lc.visible.iter().cloned());
                    lc.states = derived.into_iter().collect();
                }

                // Validate initial is in states.
                if !lc.states.contains(&lc.initial) {
                    return Err(ConfigError::Lifecycle {
                        type_name: name.clone(),
                        message: format!("initial state \"{}\" is not in states", lc.initial),
                    });
                }

                // Validate all visible entries are in states.
                for v in &lc.visible {
                    if !lc.states.contains(v) {
                        return Err(ConfigError::Lifecycle {
                            type_name: name.clone(),
                            message: format!("visible state \"{}\" is not in states", v),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use super::*;

    #[test]
    fn parse_full_config() {
        let config = DataSchema::from_yaml(indoc! {"
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
                  initial: open
                  visible: [open]
        "}).unwrap();

        assert_eq!(config.transaction_meta.len(), 3);
        assert!(config.transaction_meta["reasoning"].required);
        assert!(!config.transaction_meta["question"].required);
        assert_eq!(config.transaction_meta["reasoning"].field_type, FieldType::Text);

        let task = &config.managed_types["task"];
        assert_eq!(task.fields.len(), 5);
        assert!(task.fields["goal"].required);
        assert!(!task.fields["notes"].required);

        let lc = task.lifecycle.as_ref().unwrap();
        assert_eq!(lc.initial, "open");
        assert_eq!(lc.visible, vec!["open"]);
    }

    #[test]
    fn parse_entity_change_meta() {
        let config = DataSchema::from_yaml(indoc! {"
            entity_change_meta:
              reasoning:
                type: text
                required: true
              confidence:
                type: json
        "}).unwrap();

        assert_eq!(config.entity_change_meta.len(), 2);
        assert!(config.entity_change_meta["reasoning"].required);
        assert_eq!(config.entity_change_meta["reasoning"].field_type, FieldType::Text);
        assert!(!config.entity_change_meta["confidence"].required);
    }

    #[test]
    fn empty_config_is_valid() {
        let config = DataSchema::from_yaml("{}").unwrap();
        assert!(config.transaction_meta.is_empty());
        assert!(config.entity_change_meta.is_empty());
        assert!(config.branch_meta.is_empty());
        assert!(config.managed_types.is_empty());
    }

    #[test]
    fn parse_branch_meta() {
        let config = DataSchema::from_yaml(indoc! {"
            branch_meta:
              reasoning:
                type: text
                required: true
              purpose:
                type: text
        "}).unwrap();

        assert_eq!(config.branch_meta.len(), 2);
        assert!(config.branch_meta["reasoning"].required);
        assert!(!config.branch_meta["purpose"].required);
        assert_eq!(config.branch_meta["reasoning"].field_type, FieldType::Text);
    }

    #[test]
    fn managed_type_without_lifecycle() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              note:
                fields:
                  content: { type: text, required: true }
                  tags: { type: json }
        "}).unwrap();
        let note = &config.managed_types["note"];
        assert!(note.lifecycle.is_none());
        assert_eq!(note.fields.len(), 2);
    }

    #[test]
    fn multi_visible_lifecycle() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              review:
                fields:
                  summary: { type: text, required: true }
                lifecycle:
                  initial: draft
                  visible: [draft, in_review]
        "}).unwrap();
        let lc = config.managed_types["review"].lifecycle.as_ref().unwrap();
        assert_eq!(lc.initial, "draft");
        assert_eq!(lc.visible, vec!["draft", "in_review"]);
    }

    #[test]
    fn lifecycle_no_visible_filter() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              process:
                fields:
                  data: { type: json }
                lifecycle:
                  initial: new
        "}).unwrap();
        let lc = config.managed_types["process"].lifecycle.as_ref().unwrap();
        assert_eq!(lc.initial, "new");
        assert!(lc.visible.is_empty());
    }

    #[test]
    fn empty_initial_state() {
        let err = DataSchema::from_yaml(indoc! {r#"
            managed_types:
              bad:
                fields:
                  x: { type: text }
                lifecycle:
                  initial: ""
        "#}).unwrap_err();
        assert!(err.to_string().contains("initial state is empty"));
    }

    #[test]
    fn reserved_field_state_with_lifecycle() {
        let err = DataSchema::from_yaml(indoc! {"
            managed_types:
              bad:
                fields:
                  state: { type: text }
                lifecycle:
                  initial: open
        "}).unwrap_err();
        assert!(err.to_string().contains("reserved lifecycle field"));
    }

    #[test]
    fn field_state_ok_without_lifecycle() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              ok:
                fields:
                  state: { type: text }
        "}).unwrap();
        assert!(config.managed_types["ok"].fields.contains_key("state"));
    }

    #[test]
    fn default_field_type_is_json() {
        let config = DataSchema::from_yaml(indoc! {"
            transaction_meta:
              data:
                required: true
        "}).unwrap();
        assert_eq!(config.transaction_meta["data"].field_type, FieldType::Json);
    }

    #[test]
    fn states_derived_from_initial_and_visible() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              task:
                fields:
                  goal: { type: json, required: true }
                lifecycle:
                  initial: open
                  visible: [open]
        "}).unwrap();
        let lc = config.managed_types["task"].lifecycle.as_ref().unwrap();
        assert_eq!(lc.states, vec!["open"]);
    }

    #[test]
    fn states_derived_includes_all() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              task:
                fields:
                  goal: { type: json }
                lifecycle:
                  initial: open
                  visible: [open, in_progress]
        "}).unwrap();
        let lc = config.managed_types["task"].lifecycle.as_ref().unwrap();
        assert!(lc.states.contains(&"open".to_string()));
        assert!(lc.states.contains(&"in_progress".to_string()));
    }

    #[test]
    fn explicit_states_preserved() {
        let config = DataSchema::from_yaml(indoc! {"
            managed_types:
              task:
                fields:
                  goal: { type: json }
                lifecycle:
                  initial: open
                  states: [open, closed, archived]
                  visible: [open]
        "}).unwrap();
        let lc = config.managed_types["task"].lifecycle.as_ref().unwrap();
        assert_eq!(lc.states, vec!["open", "closed", "archived"]);
    }

    #[test]
    fn initial_not_in_explicit_states() {
        let err = DataSchema::from_yaml(indoc! {"
            managed_types:
              bad:
                fields:
                  x: { type: text }
                lifecycle:
                  initial: draft
                  states: [open, closed]
        "}).unwrap_err();
        assert!(err.to_string().contains("initial state \"draft\" is not in states"));
    }

    #[test]
    fn visible_not_in_explicit_states() {
        let err = DataSchema::from_yaml(indoc! {"
            managed_types:
              bad:
                fields:
                  x: { type: text }
                lifecycle:
                  initial: open
                  states: [open, closed]
                  visible: [open, pending]
        "}).unwrap_err();
        assert!(err.to_string().contains("visible state \"pending\" is not in states"));
    }
}
