//! Validated slug type — identity for domain objects.
//!
//! A slug is a human-readable identifier: lowercase, alphanumeric, hyphens.
//! Created only via `FromStr` / `.parse()` which enforces validation.
//! Stored as hash (UUID v5) in key, original string in value suffix.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

/// Namespace UUID for slug hashing (UUID v5).
const SLUG_HASH_NAMESPACE: Uuid = Uuid::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567890);

/// Validated, immutable slug.
///
/// Internally uses an enum to support sentinel values (Min/Max) for scan bounds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(SlugInner);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SlugInner {
    Value(String),
    Min,
    Max,
}

/// Slug validation error.
#[derive(Debug, thiserror::Error)]
pub enum SlugError {
    #[error("slug cannot be empty")]
    Empty,
    #[error("slug too long: {0} bytes (max 255)")]
    TooLong(usize),
    #[error("invalid character '{0}' in slug — allowed: a-z, 0-9, hyphen, underscore, dot")]
    InvalidChar(char),
    #[error("slug cannot start or end with a hyphen")]
    LeadingOrTrailingHyphen,
}

impl Slug {
    /// Trusted constructor for hardcoded slugs. No validation.
    pub(crate) fn new_unchecked(s: &str) -> Self {
        Self(SlugInner::Value(s.to_string()))
    }

    /// Sentinel: sorts before all real slugs (hash = `[0x00; 16]`).
    pub(crate) fn min() -> Self {
        Self(SlugInner::Min)
    }

    /// Sentinel: sorts after all real slugs (hash = `[0xFF; 16]`).
    pub(crate) fn max() -> Self {
        Self(SlugInner::Max)
    }

    /// Deterministic hash for use in storage keys.
    pub fn hash(&self) -> Uuid {
        match &self.0 {
            SlugInner::Value(s) => Uuid::new_v5(&SLUG_HASH_NAMESPACE, s.as_bytes()),
            SlugInner::Min => Uuid::from_bytes([0x00; 16]),
            SlugInner::Max => Uuid::from_bytes([0xFF; 16]),
        }
    }

    /// The original string.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            SlugInner::Value(s) => s,
            SlugInner::Min | SlugInner::Max => "",
        }
    }

    /// Byte length of the original string.
    pub fn len(&self) -> usize {
        match &self.0 {
            SlugInner::Value(s) => s.len(),
            SlugInner::Min | SlugInner::Max => 0,
        }
    }
}

impl FromStr for Slug {
    type Err = SlugError;

    fn from_str(s: &str) -> Result<Self, SlugError> {
        if s.is_empty() {
            return Err(SlugError::Empty);
        }
        if s.len() > 255 {
            return Err(SlugError::TooLong(s.len()));
        }
        if s.starts_with('-') || s.ends_with('-') {
            return Err(SlugError::LeadingOrTrailingHyphen);
        }
        for c in s.chars() {
            if !matches!(c, 'a'..='z' | '0'..='9' | '-' | '_' | '.') {
                return Err(SlugError::InvalidChar(c));
            }
        }
        Ok(Slug(SlugInner::Value(s.to_string())))
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_slugs() {
        assert!("person".parse::<Slug>().is_ok());
        assert!("entity-type".parse::<Slug>().is_ok());
        assert!("src.main.rs".parse::<Slug>().is_ok());
        assert!("my_prop_1".parse::<Slug>().is_ok());
    }

    #[test]
    fn empty_rejected() {
        assert!(matches!("".parse::<Slug>(), Err(SlugError::Empty)));
    }

    #[test]
    fn too_long_rejected() {
        let long = "a".repeat(256);
        assert!(matches!(long.parse::<Slug>(), Err(SlugError::TooLong(256))));
    }

    #[test]
    fn uppercase_rejected() {
        assert!(matches!("Person".parse::<Slug>(), Err(SlugError::InvalidChar('P'))));
    }

    #[test]
    fn spaces_rejected() {
        assert!(matches!("my entity".parse::<Slug>(), Err(SlugError::InvalidChar(' '))));
    }

    #[test]
    fn leading_hyphen_rejected() {
        assert!(matches!("-person".parse::<Slug>(), Err(SlugError::LeadingOrTrailingHyphen)));
    }

    #[test]
    fn trailing_hyphen_rejected() {
        assert!(matches!("person-".parse::<Slug>(), Err(SlugError::LeadingOrTrailingHyphen)));
    }

    #[test]
    fn hash_is_deterministic() {
        let s1: Slug = "test".parse().unwrap();
        let s2: Slug = "test".parse().unwrap();
        assert_eq!(s1.hash(), s2.hash());
    }

    #[test]
    fn different_slugs_different_hashes() {
        let s1: Slug = "alpha".parse().unwrap();
        let s2: Slug = "beta".parse().unwrap();
        assert_ne!(s1.hash(), s2.hash());
    }

    #[test]
    fn display_roundtrip() {
        let slug: Slug = "my-entity".parse().unwrap();
        assert_eq!(slug.to_string(), "my-entity");
        assert_eq!(slug.as_str(), "my-entity");
    }

    #[test]
    fn max_length_ok() {
        let s = "a".repeat(255);
        assert!(s.parse::<Slug>().is_ok());
    }

    #[test]
    fn min_hash_is_all_zeros() {
        let slug = Slug::min();
        assert_eq!(slug.hash().as_bytes(), &[0x00; 16]);
        assert_eq!(slug.as_str(), "");
        assert_eq!(slug.len(), 0);
    }

    #[test]
    fn max_hash_is_all_ones() {
        let slug = Slug::max();
        assert_eq!(slug.hash().as_bytes(), &[0xFF; 16]);
        assert_eq!(slug.as_str(), "");
        assert_eq!(slug.len(), 0);
    }
}
