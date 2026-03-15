//! Trait and macro for keys of records that live inside transactions.
//!
//! All versioned entries follow the pattern `branch(Slug) | ... | tx_id(Uuid)` —
//! branch first (for ancestry walk), tx last (for reverse scan to latest).
//! The middle is domain-specific addressing (Slug or Uuid fields).
//!
//! The macro generates a `{Name}Prefix` struct with the same layout as the key.
//! `tx_id` defaults to `Uuid::max()` (unbounded scan); use `with_transaction()`
//! to set an upper bound.

use crate::io::key_prefix::KeyPrefix;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;

/// Prefix of a versioned key.
///
/// Always encodes `branch + domain fields + tx_id`. When `tx_id` is `Uuid::max()`
/// (the default), a reverse scan starts from the very end of the prefix range —
/// equivalent to "give me the latest". When `tx_id` is a specific value, the
/// reverse scan starts from that exact point.
///
/// Composable:
/// - `with_branch(slug)` — same prefix, different branch (ancestry walk)
/// - `with_transaction(tx_id)` — set tx_id bound (bounded seek)
///
/// `Key` associated type comes from `KeyPrefix`.
pub trait VersionedPrefix: KeyPrefix {
    /// Replace tx_id, producing a bounded prefix for seeks.
    fn with_transaction(&self, tx_id: uuid::Uuid) -> Self;

    /// Clone this prefix with a different branch.
    fn with_branch(&self, branch: Slug) -> Self;
}

/// Key of a versioned, branch-scoped record.
///
/// Implemented by keys whose entries are created inside transactions
/// and follow the `branch(Slug) | domain | tx_id(Uuid)` layout.
pub trait VersionedKey: StorageKey {
    /// The prefix type — same fields, tx_id defaults to MAX.
    type Prefix: VersionedPrefix;

    fn branch(&self) -> &Slug;
    fn tx_id(&self) -> uuid::Uuid;
}

/// Declares a versioned storage key: `branch(Slug) | middle fields | tx_id(Uuid)`.
///
/// Generates:
/// - `$name` struct with `StorageKey` and `VersionedKey` impls
/// - `${name}Prefix` struct with `KeyPrefix` and `VersionedPrefix` impls
///
/// Only the middle domain fields need to be specified.
/// `branch: Slug` and `tx_id: Uuid` are added automatically.
///
/// ```ignore
/// versioned_key! {
///     pub struct EntityKey {
///         entity: Slug,
///     }
/// }
/// // Generates EntityKey (48 bytes) and EntityKeyPrefix (48 bytes, tx_id defaults to MAX)
/// ```
macro_rules! versioned_key {
    (
        $vis:vis struct $name:ident {
            $( $field:ident : $ty:ident ),* $(,)?
        }
    ) => {
        // --- Prefix struct: branch + middle fields + tx_id (defaults to MAX) ---

        ::paste::paste! {
            #[derive(Debug, Clone, PartialEq, Eq)]
            $vis struct [< $name Prefix >] {
                pub branch: $crate::io::slug::Slug,
                $( pub $field: versioned_key!(@rust_type $ty), )*
                pub tx_id: uuid::Uuid,
            }

            impl [< $name Prefix >] {
                /// Create a prefix with `tx_id = Uuid::max()` (unbounded reverse scan).
                pub fn new(branch: $crate::io::slug::Slug, $( $field: versioned_key!(@rust_type $ty), )*) -> Self {
                    Self { branch, $( $field, )* tx_id: uuid::Uuid::max() }
                }
            }

            impl $crate::io::key_prefix::KeyPrefix for [< $name Prefix >] {
                type Key = $name;
                const SIZE: usize = 16 $( + versioned_key!(@field_size $ty) )*;

                fn encode(&self) -> Vec<u8> {
                    let mut buf = vec![0u8; Self::SIZE];
                    let mut _off: usize = 0;
                    // branch hash
                    let _hash = self.branch.hash();
                    buf[_off.._off + 16].copy_from_slice(_hash.as_bytes());
                    _off += 16;
                    // middle fields
                    $( versioned_key!(@encode_fixed buf, _off, self.$field, $ty); )*
                    buf
                }

                fn encode_upper_bound(&self) -> Vec<u8> {
                    let mut buf = self.encode();
                    buf.extend_from_slice(self.tx_id.as_bytes());
                    buf
                }
            }

            impl $crate::store::versioned_key::VersionedPrefix for [< $name Prefix >] {
                fn with_transaction(&self, tx_id: uuid::Uuid) -> Self {
                    Self {
                        branch: self.branch.clone(),
                        $( $field: self.$field.clone(), )*
                        tx_id,
                    }
                }

                fn with_branch(&self, branch: $crate::io::slug::Slug) -> Self {
                    Self {
                        branch,
                        $( $field: self.$field.clone(), )*
                        tx_id: self.tx_id,
                    }
                }
            }
        }

        // --- Key struct: branch + middle fields + tx_id ---

        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            pub branch: $crate::io::slug::Slug,
            $( pub $field: versioned_key!(@rust_type $ty), )*
            pub tx_id: uuid::Uuid,
        }

        impl $crate::io::storage_key::StorageKey for $name {
            // Fixed: branch hash(16) + middle fields + tx(16)
            const SIZE: usize = 16 $( + versioned_key!(@field_size $ty) )* + 16;

            fn encode_fixed(&self) -> Vec<u8> {
                let mut buf = vec![0u8; Self::SIZE];
                let mut _off: usize = 0;
                // branch hash
                let _hash = self.branch.hash();
                buf[_off.._off + 16].copy_from_slice(_hash.as_bytes());
                _off += 16;
                // middle fields
                $( versioned_key!(@encode_fixed buf, _off, self.$field, $ty); )*
                // tx_id
                buf[_off.._off + 16].copy_from_slice(self.tx_id.as_bytes());
                buf
            }

            fn encode(&self) -> Vec<u8> {
                let _suffix_size: usize = 1 + self.branch.len()
                    $( + versioned_key!(@suffix_size self.$field, $ty) )*;
                let mut buf = self.encode_fixed();
                buf.reserve(_suffix_size);
                // slug suffixes
                // branch slug
                buf.push(self.branch.len() as u8);
                buf.extend_from_slice(self.branch.as_str().as_bytes());
                // middle slug fields
                $( versioned_key!(@encode_suffix buf, self.$field, $ty); )*
                buf
            }

            fn nil() -> Self {
                Self {
                    branch: $crate::io::slug::Slug::min(),
                    $( $field: versioned_key!(@nil_value $ty), )*
                    tx_id: uuid::Uuid::nil(),
                }
            }

            fn max() -> Self {
                Self {
                    branch: $crate::io::slug::Slug::max(),
                    $( $field: versioned_key!(@max_value $ty), )*
                    tx_id: uuid::Uuid::max(),
                }
            }

            fn decode(bytes: &[u8]) -> Result<Self, $crate::io::storage_key::KeyError> {
                if bytes.len() < Self::SIZE {
                    return Err($crate::io::storage_key::KeyError(
                        format!("{} key too short: {} < {}", stringify!($name), bytes.len(), Self::SIZE)
                    ));
                }
                let mut _off: usize = 0;
                let mut _suf: usize = Self::SIZE;

                // branch: skip hash, read from suffix
                _off += 16;
                let _len = bytes[_suf] as usize;
                _suf += 1;
                let _slug_str = std::str::from_utf8(&bytes[_suf.._suf + _len])
                    .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
                let branch: $crate::io::slug::Slug = _slug_str.parse()
                    .map_err(|e: $crate::io::slug::SlugError| $crate::io::storage_key::KeyError(e.to_string()))?;
                _suf += _len;

                // middle fields
                $( let $field = versioned_key!(@decode bytes, _off, _suf, $ty)?; )*

                // tx_id
                let tx_id = uuid::Uuid::from_slice(&bytes[_off.._off + 16])
                    .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;

                Ok(Self { branch, $( $field, )* tx_id })
            }
        }

        ::paste::paste! {
            impl $crate::store::versioned_key::VersionedKey for $name {
                type Prefix = [< $name Prefix >];
                fn branch(&self) -> &$crate::io::slug::Slug { &self.branch }
                fn tx_id(&self) -> uuid::Uuid { self.tx_id }
            }
        }
    };

    // --- Type mappings ---
    (@rust_type Uuid) => { uuid::Uuid };
    (@rust_type Slug) => { $crate::io::slug::Slug };

    // --- Fixed part sizes ---
    (@field_size Uuid) => { 16 };
    (@field_size Slug) => { 16 };

    // --- Suffix sizes (runtime) ---
    (@suffix_size $val:expr, Uuid) => { 0usize };
    (@suffix_size $val:expr, Slug) => { 1 + $val.len() };

    // --- Encode fixed part ---
    (@encode_fixed $buf:ident, $off:ident, $val:expr, Uuid) => {
        $buf[$off..$off + 16].copy_from_slice($val.as_bytes());
        $off += 16;
    };
    (@encode_fixed $buf:ident, $off:ident, $val:expr, Slug) => {
        let _hash = $val.hash();
        $buf[$off..$off + 16].copy_from_slice(_hash.as_bytes());
        $off += 16;
    };

    // --- Encode suffix ---
    (@encode_suffix $buf:ident, $val:expr, Uuid) => {};
    (@encode_suffix $buf:ident, $val:expr, Slug) => {
        $buf.push($val.len() as u8);
        $buf.extend_from_slice($val.as_str().as_bytes());
    };

    // --- Decode (two offsets: _off for fixed, _suf for suffix) ---
    (@decode $buf:ident, $off:ident, $suf:ident, Uuid) => {{
        let val = uuid::Uuid::from_slice(&$buf[$off..$off + 16])
            .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()));
        $off += 16;
        val
    }};
    (@decode $buf:ident, $off:ident, $suf:ident, Slug) => {{
        $off += 16; // skip hash
        let _len = $buf[$suf] as usize;
        $suf += 1;
        let _slug_str = std::str::from_utf8(&$buf[$suf..$suf + _len])
            .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
        $suf += _len;
        let val: Result<$crate::io::slug::Slug, $crate::io::storage_key::KeyError> = _slug_str.parse()
            .map_err(|e: $crate::io::slug::SlugError| $crate::io::storage_key::KeyError(e.to_string()));
        val
    }};

    // --- Nil (minimum) values ---
    (@nil_value Uuid) => { uuid::Uuid::nil() };
    (@nil_value Slug) => { $crate::io::slug::Slug::min() };

    // --- Max values ---
    (@max_value Uuid) => { uuid::Uuid::max() };
    (@max_value Slug) => { $crate::io::slug::Slug::max() };
}

pub(crate) use versioned_key;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::key_prefix::KeyPrefix;
    use uuid::Uuid;

    versioned_key! {
        pub struct SlugMiddleKey {
            entity_id: Slug,
        }
    }

    versioned_key! {
        pub struct UuidMiddleKey {
            change_id: Uuid,
        }
    }

    versioned_key! {
        pub struct EmptyMiddleKey {}
    }

    versioned_key! {
        pub struct MultiSlugKey {
            type_name: Slug,
            prop_name: Slug,
        }
    }

    versioned_key! {
        pub struct MixedKey {
            type_name: Slug,
            seq: Uuid,
        }
    }

    #[test]
    fn auto_size() {
        assert_eq!(EmptyMiddleKey::SIZE, 32);
        assert_eq!(SlugMiddleKey::SIZE, 48);
        assert_eq!(UuidMiddleKey::SIZE, 48);
        assert_eq!(MultiSlugKey::SIZE, 64);
        assert_eq!(MixedKey::SIZE, 64);
    }

    #[test]
    fn prefix_size() {
        assert_eq!(EmptyMiddleKeyPrefix::SIZE, 16);
        assert_eq!(SlugMiddleKeyPrefix::SIZE, 32);
        assert_eq!(UuidMiddleKeyPrefix::SIZE, 32);
        assert_eq!(MultiSlugKeyPrefix::SIZE, 48);
        assert_eq!(MixedKeyPrefix::SIZE, 48);
    }

    #[test]
    fn encode_is_domain_only() {
        let branch: Slug = "main".parse().unwrap();
        let entity: Slug = "my-entity".parse().unwrap();

        let key = SlugMiddleKey {
            branch: branch.clone(),
            entity_id: entity.clone(),
            tx_id: Uuid::from_u128(3),
        };
        let prefix = SlugMiddleKeyPrefix::new(branch, entity);

        let key_bytes = key.encode();
        let prefix_bytes = prefix.encode();

        // encode() is domain part only (no tx_id)
        assert_eq!(prefix_bytes.len(), SlugMiddleKeyPrefix::SIZE);
        assert_eq!(&key_bytes[..prefix_bytes.len()], &prefix_bytes[..]);
    }

    #[test]
    fn upper_bound_includes_tx_id() {
        let prefix = SlugMiddleKeyPrefix::new(
            "main".parse().unwrap(),
            "entity".parse().unwrap(),
        );
        let upper = prefix.encode_upper_bound();
        // encode() = 32 bytes, upper_bound = 32 + 16 = 48
        assert_eq!(upper.len(), SlugMiddleKey::SIZE);
        // Default tx_id = MAX → last 16 bytes are 0xFF
        assert!(upper[32..].iter().all(|&b| b == 0xFF));

        // With specific tx_id
        let bounded = prefix.with_transaction(Uuid::from_u128(42));
        let upper = bounded.encode_upper_bound();
        assert_eq!(&upper[32..], Uuid::from_u128(42).as_bytes());
    }

    #[test]
    fn default_prefix_has_max_tx_id() {
        let prefix = SlugMiddleKeyPrefix::new(
            "main".parse().unwrap(),
            "entity".parse().unwrap(),
        );
        assert_eq!(prefix.tx_id, Uuid::max());
    }

    #[test]
    fn roundtrip_slug_middle() {
        let branch: Slug = "main".parse().unwrap();
        let entity: Slug = "my-entity".parse().unwrap();
        let key = SlugMiddleKey {
            branch: branch.clone(),
            entity_id: entity.clone(),
            tx_id: Uuid::from_u128(3),
        };
        let bytes = key.encode();
        assert!(bytes.len() > SlugMiddleKey::SIZE); // has suffix
        let decoded = SlugMiddleKey::decode(&bytes).unwrap();
        assert_eq!(decoded.branch, branch);
        assert_eq!(decoded.entity_id, entity);
        assert_eq!(decoded.tx_id, Uuid::from_u128(3));
    }

    #[test]
    fn roundtrip_uuid_middle() {
        let branch: Slug = "main".parse().unwrap();
        let key = UuidMiddleKey {
            branch: branch.clone(),
            change_id: Uuid::from_u128(42),
            tx_id: Uuid::from_u128(3),
        };
        let bytes = key.encode();
        let decoded = UuidMiddleKey::decode(&bytes).unwrap();
        assert_eq!(decoded.branch, branch);
        assert_eq!(decoded.change_id, Uuid::from_u128(42));
    }

    #[test]
    fn roundtrip_empty_middle() {
        let branch: Slug = "exploration".parse().unwrap();
        let key = EmptyMiddleKey {
            branch: branch.clone(),
            tx_id: Uuid::from_u128(1),
        };
        let bytes = key.encode();
        let decoded = EmptyMiddleKey::decode(&bytes).unwrap();
        assert_eq!(decoded.branch, branch);
        assert_eq!(decoded.tx_id, Uuid::from_u128(1));
    }

    #[test]
    fn roundtrip_multi_slug() {
        let branch: Slug = "main".parse().unwrap();
        let t: Slug = "person".parse().unwrap();
        let p: Slug = "age".parse().unwrap();
        let key = MultiSlugKey {
            branch: branch.clone(),
            type_name: t.clone(),
            prop_name: p.clone(),
            tx_id: Uuid::from_u128(1),
        };
        let bytes = key.encode();
        let decoded = MultiSlugKey::decode(&bytes).unwrap();
        assert_eq!(decoded.branch, branch);
        assert_eq!(decoded.type_name, t);
        assert_eq!(decoded.prop_name, p);
    }

    #[test]
    fn roundtrip_mixed() {
        let branch: Slug = "main".parse().unwrap();
        let t: Slug = "task".parse().unwrap();
        let key = MixedKey {
            branch: branch.clone(),
            type_name: t.clone(),
            seq: Uuid::from_u128(99),
            tx_id: Uuid::from_u128(1),
        };
        let bytes = key.encode();
        let decoded = MixedKey::decode(&bytes).unwrap();
        assert_eq!(decoded.type_name, t);
        assert_eq!(decoded.seq, Uuid::from_u128(99));
    }

    #[test]
    fn versioned_key_trait() {
        let branch: Slug = "main".parse().unwrap();
        let key = SlugMiddleKey {
            branch: branch.clone(),
            entity_id: "test".parse().unwrap(),
            tx_id: Uuid::from_u128(30),
        };
        assert_eq!(key.branch(), &branch);
        assert_eq!(key.tx_id(), Uuid::from_u128(30));
    }

    #[test]
    fn with_transaction_replaces_tx_id() {
        let prefix = SlugMiddleKeyPrefix::new(
            "main".parse().unwrap(),
            "my-entity".parse().unwrap(),
        );
        let tx_id = Uuid::from_u128(42);
        let bounded = prefix.with_transaction(tx_id);

        assert_eq!(bounded.tx_id, tx_id);
        assert_eq!(bounded.branch, prefix.branch);
        assert_eq!(bounded.entity_id, prefix.entity_id);
    }

    #[test]
    fn with_branch_changes_branch() {
        let prefix = SlugMiddleKeyPrefix::new(
            "main".parse().unwrap(),
            "entity-1".parse().unwrap(),
        );
        let rebound = prefix.with_branch("child".parse().unwrap());

        assert_eq!(rebound.branch.as_str(), "child");
        assert_eq!(rebound.entity_id, prefix.entity_id);
        assert_eq!(rebound.tx_id, prefix.tx_id);
        assert_ne!(rebound.encode(), prefix.encode());
    }

    #[test]
    fn fixed_part_sorts_by_hash() {
        let branch: Slug = "main".parse().unwrap();
        let k1 = SlugMiddleKey {
            branch: branch.clone(),
            entity_id: "alpha".parse().unwrap(),
            tx_id: Uuid::from_u128(1),
        };
        let k2 = SlugMiddleKey {
            branch: branch.clone(),
            entity_id: "beta".parse().unwrap(),
            tx_id: Uuid::from_u128(1),
        };
        let e1 = k1.encode();
        let e2 = k2.encode();
        // Fixed parts (first SIZE bytes) differ by hash
        assert_ne!(&e1[..SlugMiddleKey::SIZE], &e2[..SlugMiddleKey::SIZE]);
    }
}
