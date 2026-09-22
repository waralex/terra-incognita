use super::{Block, Transaction};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;
use crate::io::{DbError, DbItem};
use serde::{Deserialize, Serialize};

storage_key! { pub struct HeadKey { block: Uuid } }
storage_key! { pub struct VersionKey { block: Uuid, tx: Uuid } }
storage_key! { pub struct ChildKey { parent: Uuid, position: i64, child: Uuid } }
storage_key! { pub struct ChildVersionKey { parent: Uuid, child: Uuid, tx: Uuid } }
storage_key! { pub struct TxKey { tx: Uuid } }
storage_key! { pub struct ChangeKey { tx: Uuid, block: Uuid } }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Membership {
    pub present: bool,
    pub position: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {}
macro_rules! record {
    ($name:ident, $key:ty, $value:ty, $cf:literal) => {
        pub struct $name {
            pub key: $key,
            pub value: $value,
        }
        impl DbItem for $name {
            const FORMAT_VERSION: u32 = 2;
            type Key = $key;
            type Value = $value;
            fn cf() -> &'static str {
                $cf
            }
            fn key(&self) -> &Self::Key {
                &self.key
            }
            fn value(&self) -> &Self::Value {
                &self.value
            }
            fn from_parts(key: Self::Key, value: Self::Value) -> Self {
                Self { key, value }
            }
        }
    };
}
macro_rules! json_value {
    ($t:ty) => {
        impl StorageValue for $t {
            fn encode(&self) -> Result<Vec<u8>, DbError> {
                serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
            }
            fn decode(bytes: &[u8]) -> Result<Self, DbError> {
                serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
            }
        }
    };
}
json_value!(Block);
json_value!(Transaction);
json_value!(Membership);
json_value!(Marker);
record!(Head, HeadKey, Block, "doc_blocks");
record!(Version, VersionKey, Block, "doc_block_versions");
record!(Child, ChildKey, Marker, "doc_children");
record!(
    ChildVersion,
    ChildVersionKey,
    Membership,
    "doc_child_versions"
);
record!(Tx, TxKey, Transaction, "doc_transactions");
record!(Change, ChangeKey, Marker, "doc_transaction_changes");
