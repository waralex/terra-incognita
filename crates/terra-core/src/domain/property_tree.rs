//! Lossless structural view of already-selected property assertions.
//!
//! This module does not choose versions, resolve statuses, or interpret JSON values.
//! Dots in property names delimit paths. Nodes can have both assertions and children.
use crate::domain::property_path;
use std::collections::BTreeMap;

use super::entity::PropertyValue;

pub type PropertyTree<M> = BTreeMap<String, PropertyNode<M>>;

#[derive(Debug, Clone)]
pub struct PropertyNode<M> {
    pub assertions: Vec<PropertyValue<M>>,
    pub children: PropertyTree<M>,
}

impl<M> Default for PropertyNode<M> {
    fn default() -> Self {
        Self {
            assertions: Vec::new(),
            children: BTreeMap::new(),
        }
    }
}

/// Preserve assertion order within each exact property, including all overlays.
/// Empty segments are preserved too: legacy Slugs allow repeated/edge dots.
pub fn property_tree<M>(properties: impl IntoIterator<Item = PropertyValue<M>>) -> PropertyTree<M> {
    let mut root = PropertyTree::new();
    for property in properties {
        let path = property.property.as_str().to_owned();
        let mut parts = property_path::segments(&path).peekable();
        let mut children = &mut root;
        while let Some(part) = parts.next() {
            let node = children.entry(part.to_owned()).or_default();
            if parts.peek().is_none() {
                node.assertions.push(property);
                break;
            }
            children = &mut node.children;
        }
    }
    root
}

/// Select an exact path and its descendants, never lexical siblings such as `ab` for `a`.
pub fn in_property_subtree(property: &str, prefix: &str) -> bool {
    property_path::contains(property, prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn p(path: &str, n: usize) -> PropertyValue<usize> {
        PropertyValue {
            supersedes_tx: None,
            property: path.parse().unwrap(),
            value: json!(n),
            context: n,
        }
    }
    #[test]
    fn preserves_overlays_parent_values_and_reserved_names() {
        let tree = property_tree([
            p("a", 1),
            p("a.b.c", 2),
            p("a.b.c", 3),
            p("a.children.assertions", 4),
            p("a..", 5),
        ]);
        assert_eq!(tree["a"].assertions[0].value, json!(1));
        let leaf = &tree["a"].children["b"].children["c"].assertions;
        assert_eq!(
            leaf.iter().map(|p| p.context).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(
            tree["a"].children["children"].children["assertions"].assertions[0].context,
            4
        );
        assert_eq!(tree["a"].children[""].children[""].assertions[0].context, 5);
        assert!(in_property_subtree("a.b", "a"));
        assert!(in_property_subtree("a", "a"));
        assert!(!in_property_subtree("ab", "a"));
        assert!(property_tree::<()>([]).is_empty());
    }
    #[test]
    fn supports_depth_up_to_slug_length_limit() {
        let path = vec!["a"; 128].join(".");
        let tree = property_tree([p(&path, 1)]);
        let mut node = &tree["a"];
        for _ in 1..128 {
            node = &node.children["a"];
        }
        assert_eq!(node.assertions.len(), 1);
    }
}
