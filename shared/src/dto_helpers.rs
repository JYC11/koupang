use chrono::{DateTime, Utc};
use uuid::Uuid;

pub fn fmt_id(id: &Uuid) -> String {
    id.to_string()
}

pub fn fmt_datetime(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

pub fn fmt_datetime_opt(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(fmt_datetime)
}
