//! Click event model for asynchronous click tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A click event published to Kafka and consumed into ClickHouse.
///
/// `link_id` is resolved at redirect time (from the loaded link or the cache),
/// so the consumer never needs to touch PostgreSQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickEvent {
    pub link_id: i64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub clicked_at: DateTime<Utc>,
}

impl ClickEvent {
    /// Creates a new click event.
    pub fn new(
        link_id: i64,
        ip: Option<String>,
        user_agent: Option<String>,
        referer: Option<String>,
        clicked_at: DateTime<Utc>,
    ) -> Self {
        Self {
            link_id,
            ip,
            user_agent,
            referer,
            clicked_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample() -> ClickEvent {
        ClickEvent::new(
            42,
            Some("192.168.1.1".to_string()),
            Some("Mozilla/5.0".to_string()),
            Some("https://google.com".to_string()),
            Utc.with_ymd_and_hms(2026, 6, 19, 12, 0, 0).unwrap(),
        )
    }

    #[test]
    fn test_click_event_fields() {
        let e = sample();
        assert_eq!(e.link_id, 42);
        assert_eq!(e.ip.as_deref(), Some("192.168.1.1"));
        assert_eq!(e.user_agent.as_deref(), Some("Mozilla/5.0"));
        assert_eq!(e.referer.as_deref(), Some("https://google.com"));
    }

    #[test]
    fn test_click_event_json_round_trip() {
        let e = sample();
        let json = serde_json::to_string(&e).unwrap();
        let back: ClickEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.link_id, e.link_id);
        assert_eq!(back.ip, e.ip);
        assert_eq!(back.user_agent, e.user_agent);
        assert_eq!(back.referer, e.referer);
        assert_eq!(back.clicked_at, e.clicked_at);
    }

    #[test]
    fn test_click_event_minimal_serialization() {
        let e = ClickEvent::new(
            7,
            None,
            None,
            None,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: ClickEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.link_id, 7);
        assert!(back.ip.is_none());
    }
}
