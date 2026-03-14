//! Transaction metadata attached to domain objects on read.

use uuid::Uuid;

use crate::io::Slug;

/// Transaction provenance — attached to domain objects when reading.
///
/// Used as the generic parameter `M` in domain types:
/// `Entity<()>` for write input, `Entity<TxMeta>` for read output.
#[derive(Debug, Clone)]
pub struct TxMeta {
    pub tx_id: Uuid,
    pub branch: Slug,
}
