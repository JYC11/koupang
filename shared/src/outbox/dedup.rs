use redis::AsyncCommands;
use uuid::Uuid;

const KEY_PREFIX: &str = "outbox:published:";
const TTL_SECS: u64 = 300; // 5 minutes

fn dedup_key(event_id: &Uuid) -> String {
    format!("{KEY_PREFIX}{event_id}")
}

/// Check if an event was already published (Redis dedup cache hit).
/// Returns false if Redis is unavailable — fail-open to avoid blocking the relay.
pub async fn is_published(conn: &redis::aio::ConnectionManager, event_id: &Uuid) -> bool {
    let key = dedup_key(event_id);
    match conn.clone().exists::<_, bool>(&key).await {
        Ok(exists) => exists,
        Err(e) => {
            tracing::warn!(event_id = %event_id, error = %e, "Redis dedup check failed, proceeding with publish");
            false
        }
    }
}

/// Mark an event as published in the Redis dedup cache.
/// Silently no-ops on Redis failure — dedup is best-effort.
pub async fn mark_published(conn: &redis::aio::ConnectionManager, event_id: &Uuid) {
    let key = dedup_key(event_id);
    if let Err(e) = conn.clone().set_ex::<_, _, ()>(&key, "1", TTL_SECS).await {
        tracing::warn!(event_id = %event_id, error = %e, "Redis dedup mark failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dedup_key_format() {
        let id = Uuid::nil();
        assert_eq!(
            dedup_key(&id),
            "outbox:published:00000000-0000-0000-0000-000000000000"
        );
    }
}
