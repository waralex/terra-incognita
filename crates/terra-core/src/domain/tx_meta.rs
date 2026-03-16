//! Transaction metadata attached to domain objects on read.

use chrono::{DateTime, Utc};
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
    pub reasoning: Option<String>,
    /// Timestamp extracted from UUID v7 tx_id.
    pub time: Option<DateTime<Utc>>,
}

/// Extract timestamp from a UUID v7.
pub fn time_from_uuid(uuid: Uuid) -> Option<DateTime<Utc>> {
    let ts = uuid.get_timestamp()?;
    let (secs, nanos) = ts.to_unix();
    DateTime::from_timestamp(secs as i64, nanos as u32)
}
