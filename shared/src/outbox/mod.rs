pub mod dedup;
mod metrics;
mod processed;
mod relay;
mod repository;
pub(crate) mod types;

pub use crate::config::relay_config::RelayConfig;
pub use metrics::*;
pub use processed::*;
pub use relay::OutboxRelay;
pub use repository::*;
pub use types::{
    // Failure escalation
    FailureEscalation,
    LogFailureEscalation,
    OutboxEvent,
    OutboxInsert,
    OutboxMetrics,
    OutboxStatus,
    // Relay heartbeat
    RelayHeartbeat,
    // Trace context
    capture_trace_context,
};
