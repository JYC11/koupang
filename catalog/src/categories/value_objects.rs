use once_cell::sync::Lazy;
use regex::Regex;
use shared::errors::AppError;
use std::fmt;

static LTREE_LABEL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z][a-z0-9_]*$").unwrap());

// ── CategoryName (via macro) ──────────────────────────────

crate::validated_name!(CategoryName, "Category name", 255);

// ── LtreeLabel ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LtreeLabel(String);

impl LtreeLabel {
    pub fn new(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                "Ltree label must not be empty".to_string(),
            ));
        }

        if trimmed.len() > 255 {
            return Err(AppError::BadRequest(
                "Ltree label must not exceed 255 characters".to_string(),
            ));
        }

        if !LTREE_LABEL_RE.is_match(trimmed) {
            return Err(AppError::BadRequest(
                "Ltree label must be lowercase alphanumeric with underscores, starting with a letter (e.g. electronics, smart_phones)".to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Generate a label from a human-readable name.
    /// "Smart Phones" -> "smart_phones", "Electronics & Gadgets!" -> "electronics_gadgets"
    pub fn from_name(name: &str) -> Result<Self, AppError> {
        let label: String = name
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();

        let label = label
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_");

        if label.is_empty() {
            return Err(AppError::BadRequest(
                "Cannot generate ltree label from empty name".to_string(),
            ));
        }

        // Ensure starts with a letter (strip leading digits/underscores)
        let label = label.trim_start_matches(|c: char| !c.is_ascii_lowercase());

        Self::new(label)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for LtreeLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── CategoryName tests ────────────────────────────────

    #[test]
    fn category_name_valid() {
        assert!(CategoryName::new("Electronics").is_ok());
        assert!(CategoryName::new("A").is_ok());
    }

    #[test]
    fn category_name_trims_whitespace() {
        let name = CategoryName::new("  Electronics  ").unwrap();
        assert_eq!(name.as_str(), "Electronics");
    }

    #[test]
    fn category_name_rejects_empty() {
        assert!(CategoryName::new("").is_err());
        assert!(CategoryName::new("   ").is_err());
    }

    #[test]
    fn category_name_rejects_too_long() {
        let long = "a".repeat(256);
        assert!(CategoryName::new(&long).is_err());
    }

    #[test]
    fn category_name_accepts_max_length() {
        let max = "a".repeat(255);
        assert!(CategoryName::new(&max).is_ok());
    }

    // ── LtreeLabel tests ──────────────────────────────────

    #[test]
    fn ltree_label_valid() {
        assert!(LtreeLabel::new("electronics").is_ok());
        assert!(LtreeLabel::new("smart_phones").is_ok());
        assert!(LtreeLabel::new("a").is_ok());
        assert!(LtreeLabel::new("a1b2").is_ok());
    }

    #[test]
    fn ltree_label_rejects_empty() {
        assert!(LtreeLabel::new("").is_err());
        assert!(LtreeLabel::new("   ").is_err());
    }

    #[test]
    fn ltree_label_rejects_uppercase() {
        assert!(LtreeLabel::new("Electronics").is_err());
    }

    #[test]
    fn ltree_label_rejects_starting_with_digit() {
        assert!(LtreeLabel::new("1electronics").is_err());
    }

    #[test]
    fn ltree_label_rejects_special_chars() {
        assert!(LtreeLabel::new("smart-phones").is_err());
        assert!(LtreeLabel::new("hello world").is_err());
    }

    #[test]
    fn ltree_label_rejects_too_long() {
        let long = format!("a{}", "b".repeat(255));
        assert!(LtreeLabel::new(&long).is_err());
    }

    #[test]
    fn ltree_label_from_name() {
        let label = LtreeLabel::from_name("Smart Phones").unwrap();
        assert_eq!(label.as_str(), "smart_phones");
    }

    #[test]
    fn ltree_label_from_name_with_special_chars() {
        let label = LtreeLabel::from_name("Electronics & Gadgets!").unwrap();
        assert_eq!(label.as_str(), "electronics_gadgets");
    }

    #[test]
    fn ltree_label_from_name_collapses_underscores() {
        let label = LtreeLabel::from_name("Hello   World").unwrap();
        assert_eq!(label.as_str(), "hello_world");
    }

    #[test]
    fn ltree_label_from_name_rejects_empty() {
        assert!(LtreeLabel::from_name("").is_err());
        assert!(LtreeLabel::from_name("   ").is_err());
    }
}
