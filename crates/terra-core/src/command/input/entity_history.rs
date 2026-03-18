//! EntityHistoryQuery — parameters for listing entity history.

use uuid::Uuid;

use crate::io::slug::Slug;

/// Parameters for listing entity change history.
///
/// Two pagination modes:
/// - **Cursor**: `at_tx` (upper bound) + `limit`
/// - **Range**: `tx_id_from`..=`tx_id_to` (capped by `limit`)
///
/// Mode detection: if `tx_id_from` is Some → range mode, otherwise cursor mode.
pub struct EntityHistoryQuery {
    /// Entity slug to get history for.
    pub entity: Slug,
    /// Filter: only transactions where this property changed.
    pub property: Option<Slug>,
    /// Upper bound for cursor mode (default: head_tx).
    pub at_tx: Option<Uuid>,
    /// Max entries to return.
    pub limit: usize,
    /// Lower bound (inclusive) for range mode.
    pub tx_id_from: Option<Uuid>,
    /// Upper bound (inclusive) for range mode (default: head_tx).
    pub tx_id_to: Option<Uuid>,
}

impl EntityHistoryQuery {
    pub fn new(entity: Slug, limit: usize) -> Self {
        Self {
            entity,
            property: None,
            at_tx: None,
            limit,
            tx_id_from: None,
            tx_id_to: None,
        }
    }

    pub fn with_property(mut self, property: Slug) -> Self {
        self.property = Some(property);
        self
    }

    pub fn with_at_tx(mut self, at_tx: Uuid) -> Self {
        self.at_tx = Some(at_tx);
        self
    }

    pub fn with_range(mut self, from: Uuid, to: Uuid) -> Self {
        self.tx_id_from = Some(from);
        self.tx_id_to = Some(to);
        self
    }
}
