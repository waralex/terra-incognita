//! CheckoutInput — command describing branch creation with a first transaction.

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::command::input::transaction::TransactionInput;
use crate::io::slug::Slug;

/// Branch creation command — always includes a first transaction.
///
/// A branch without a transaction is meaningless: the transaction
/// establishes the initial state on the new branch.
pub struct CheckoutInput {
    pub(crate) slug: Slug,
    pub(crate) meta: Map<String, Value>,
    pub(crate) created_from_tx: Option<Uuid>,
    pub(crate) transaction: TransactionInput,
}

impl CheckoutInput {
    /// Create a checkout: branch slug, branch metadata, optional branch point,
    /// and the first transaction to execute on the new branch.
    ///
    /// If `created_from_tx` is None, the latest transaction on the parent
    /// branch is used as the branch point.
    pub fn new(
        slug: Slug,
        meta: Map<String, Value>,
        created_from_tx: Option<Uuid>,
        transaction: TransactionInput,
    ) -> Self {
        Self {
            slug,
            meta,
            created_from_tx,
            transaction,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

    #[test]
    fn checkout_without_branch_point() {
        let input = CheckoutInput::new(
            "feature".parse().unwrap(),
            meta("explore"),
            None,
            TransactionInput::new(meta("init")),
        );
        assert_eq!(input.slug.as_str(), "feature");
        assert_eq!(input.meta["reasoning"], "explore");
        assert!(input.created_from_tx.is_none());
        assert_eq!(input.transaction.meta["reasoning"], "init");
    }

    #[test]
    fn checkout_with_branch_point() {
        let tx_id = Uuid::now_v7();
        let input = CheckoutInput::new(
            "feature".parse().unwrap(),
            meta("explore"),
            Some(tx_id),
            TransactionInput::new(meta("init")),
        );
        assert_eq!(input.created_from_tx, Some(tx_id));
    }
}
