use crate::schema::SchemaError;

/// Validates a slug: lowercase ASCII, digits, hyphens, underscores;
/// no leading/trailing/consecutive hyphens or underscores.
pub fn validate_slug(slug: &str) -> Result<(), SchemaError> {
    if slug.is_empty() {
        return Err(SchemaError::InvalidSlug(slug.to_string()));
    }

    let valid = slug.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_' || b.is_ascii_digit())
        && !slug.starts_with('-')
        && !slug.starts_with('_')
        && !slug.ends_with('-')
        && !slug.ends_with('_')
        && !slug.contains("--")
        && !slug.contains("__");

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
        assert!(validate_slug("terrain_suitability").is_ok());
        assert!(validate_slug("mixed-and_styles").is_ok());
    }

    #[test]
    fn invalid_slugs() {
        assert!(validate_slug("").is_err());
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("_leading").is_err());
        assert!(validate_slug("trailing-").is_err());
        assert!(validate_slug("trailing_").is_err());
        assert!(validate_slug("double--hyphen").is_err());
        assert!(validate_slug("double__underscore").is_err());
        assert!(validate_slug("Upper").is_err());
        assert!(validate_slug("has space").is_err());
    }
}
