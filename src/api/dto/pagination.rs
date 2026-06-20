//! Pagination and filtering query parameters.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};

/// Pagination query parameters.
///
/// Uses `serde_with` to parse page numbers from query strings as integers.
#[serde_as]
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub page: Option<u32>,

    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub page_size: Option<u32>,
}

impl PaginationParams {
    /// Validates pagination parameters and converts to database offset/limit.
    ///
    /// # Defaults
    ///
    /// - `page`: 1
    /// - `page_size`: 25
    ///
    /// # Validation
    ///
    /// - Page must be > 0
    /// - Page size must be between 10 and 1000
    ///
    /// # Returns
    ///
    /// `(offset, limit)` tuple for SQL queries.
    pub fn validate_and_get_offset_limit(&self) -> Result<(i64, i64), String> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(25);

        if page == 0 {
            return Err("Page must be greater than 0".to_string());
        }

        if !(10..=1000).contains(&page_size) {
            return Err("Page size must be between 10 and 1000".to_string());
        }

        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;

        Ok((offset, limit))
    }
}

/// Date range filtering parameters.
#[derive(Debug, Deserialize)]
pub struct DateFilterParams {
    #[serde(default, with = "optional_rfc3339")]
    pub from: Option<DateTime<Utc>>,

    #[serde(default, with = "optional_rfc3339")]
    pub to: Option<DateTime<Utc>>,
}

/// Custom Serde deserializer for RFC3339 datetime strings.
mod optional_rfc3339 {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.with_timezone(&Utc)))
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Combined query parameters for statistics endpoints.
#[serde_as]
#[derive(Debug, Deserialize)]
pub struct StatsQueryParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    #[serde(flatten)]
    pub date_filter: DateFilterParams,

    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub domain_id: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(page: Option<u32>, page_size: Option<u32>) -> PaginationParams {
        PaginationParams { page, page_size }
    }

    #[test]
    fn test_defaults() {
        let (offset, limit) = params(None, None).validate_and_get_offset_limit().unwrap();
        assert_eq!(offset, 0);
        assert_eq!(limit, 25);
    }

    #[test]
    fn test_page_2_with_default_size() {
        let (offset, limit) = params(Some(2), None)
            .validate_and_get_offset_limit()
            .unwrap();
        assert_eq!(offset, 25);
        assert_eq!(limit, 25);
    }

    #[test]
    fn test_custom_page_and_size() {
        let (offset, limit) = params(Some(3), Some(50))
            .validate_and_get_offset_limit()
            .unwrap();
        assert_eq!(offset, 100);
        assert_eq!(limit, 50);
    }

    #[test]
    fn test_page_zero_is_error() {
        assert!(
            params(Some(0), None)
                .validate_and_get_offset_limit()
                .is_err()
        );
    }

    #[test]
    fn test_page_size_below_minimum_is_error() {
        assert!(
            params(None, Some(9))
                .validate_and_get_offset_limit()
                .is_err()
        );
        assert!(
            params(None, Some(0))
                .validate_and_get_offset_limit()
                .is_err()
        );
    }

    #[test]
    fn test_page_size_at_minimum_is_ok() {
        assert!(
            params(None, Some(10))
                .validate_and_get_offset_limit()
                .is_ok()
        );
    }

    #[test]
    fn test_page_size_at_maximum_is_ok() {
        assert!(
            params(None, Some(1000))
                .validate_and_get_offset_limit()
                .is_ok()
        );
    }

    #[test]
    fn test_page_size_above_maximum_is_error() {
        assert!(
            params(None, Some(1001))
                .validate_and_get_offset_limit()
                .is_err()
        );
    }

    #[test]
    fn test_optional_rfc3339_deserializer() {
        let json = r#"{"from": "2026-01-01T00:00:00Z", "to": null}"#;
        let p: DateFilterParams = serde_json::from_str(json).unwrap();
        assert!(p.from.is_some());
        assert!(p.to.is_none());
    }

    #[test]
    fn test_optional_rfc3339_both_absent() {
        let p: DateFilterParams = serde_json::from_str("{}").unwrap();
        assert!(p.from.is_none());
        assert!(p.to.is_none());
    }

    #[test]
    fn test_optional_rfc3339_invalid_format_is_error() {
        let json = r#"{"from": "not-a-date"}"#;
        assert!(serde_json::from_str::<DateFilterParams>(json).is_err());
    }
}
