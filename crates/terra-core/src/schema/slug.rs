use crate::schema::SchemaError;

/// Validates a slug: lowercase ASCII, digits, hyphens; no leading/trailing/consecutive hyphens.
pub fn validate_slug(slug: &str) -> Result<(), SchemaError> {
    if slug.is_empty() {
        return Err(SchemaError::InvalidSlug(slug.to_string()));
    }

    let valid = slug.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b.is_ascii_digit())
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--");

    if !valid {
        return Err(SchemaError::InvalidSlug(slug.to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_slugs() {
        assert!(validate_slug("research-project").is_ok());
        assert!(validate_slug("unit").is_ok());
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug("unit-123").is_ok());
    }

    #[test]
    fn invalid_slugs() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("trailing-").is_err());
        assert!(validate_slug("double--hyphen").is_err());
        assert!(validate_slug("Upper").is_err());
        assert!(validate_slug("has space").is_err());
        assert!(validate_slug("under_score").is_err());
    }
}
