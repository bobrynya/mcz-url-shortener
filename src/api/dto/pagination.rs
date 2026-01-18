use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};

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
    pub fn validate_and_get_offset_limit(&self) -> Result<(i64, i64), String> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(25);

        if page == 0 {
            return Err("Page must be greater than 0".to_string());
        }

        if !(10..=50).contains(&page_size) {
            return Err("Page size must be between 10 and 50".to_string());
        }

        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;

        Ok((offset, limit))
    }
}

#[derive(Debug, Deserialize)]
pub struct DateFilterParams {
    #[serde(default, with = "optional_rfc3339")]
    pub from: Option<DateTime<Utc>>,

    #[serde(default, with = "optional_rfc3339")]
    pub to: Option<DateTime<Utc>>,
}

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

#[derive(Debug, Deserialize)]
pub struct StatsQueryParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    #[serde(flatten)]
    pub date_filter: DateFilterParams,

    pub domain: Option<String>,
}
