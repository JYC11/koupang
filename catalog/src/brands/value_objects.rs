pub use crate::common::value_objects::HttpUrl;

shared::validated_name!(BrandName, "Brand name", 255);
shared::valid_id!(BrandId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_name_valid() {
        assert!(BrandName::new("Samsung").is_ok());
        assert!(BrandName::new("A").is_ok());
        assert!(BrandName::new("LG Electronics").is_ok());
    }

    #[test]
    fn brand_name_trims_whitespace() {
        let name = BrandName::new("  Samsung  ").unwrap();
        assert_eq!(name.as_str(), "Samsung");
    }

    #[test]
    fn brand_name_rejects_empty() {
        assert!(BrandName::new("").is_err());
        assert!(BrandName::new("   ").is_err());
    }

    #[test]
    fn brand_name_rejects_too_long() {
        let long = "a".repeat(256);
        assert!(BrandName::new(&long).is_err());
    }

    #[test]
    fn brand_name_accepts_max_length() {
        let max = "a".repeat(255);
        assert!(BrandName::new(&max).is_ok());
    }
}
