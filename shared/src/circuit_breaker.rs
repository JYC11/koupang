use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

// ── Configuration ─────────────────────────────────────────────

pub struct CircuitBreakerConfig {
    /// Number of recent calls tracked in the sliding window.
    pub window_size: usize,
    /// Fraction of failures (0.0–1.0) that trips the breaker.
    pub failure_threshold: f64,
    /// How long the breaker stays open before allowing a probe.
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            window_size: 10,
            failure_threshold: 0.5,
            cooldown: Duration::from_secs(30),
        }
    }
}

// ── State ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerStatus {
    Closed,
    Open,
    HalfOpen,
}

impl fmt::Display for BreakerStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half-open"),
        }
    }
}

struct BreakerState {
    status: BreakerStatus,
    /// true = success, false = retryable failure. Business declines are not recorded.
    window: VecDeque<bool>,
    opened_at: Option<Instant>,
}

impl BreakerState {
    fn new(capacity: usize) -> Self {
        Self {
            status: BreakerStatus::Closed,
            window: VecDeque::with_capacity(capacity),
            opened_at: None,
        }
    }
}

// ── Error ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CircuitOpenError;

impl fmt::Display for CircuitOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circuit breaker is open — call rejected")
    }
}

impl std::error::Error for CircuitOpenError {}

// ── Circuit breaker ───────────────────────────────────────────

/// Generic circuit breaker with a count-based sliding window.
///
/// The caller drives the state machine:
/// 1. `check()` before each call — rejects if circuit is open.
/// 2. `record_success()` or `record_retryable_failure()` after the call.
/// 3. For business errors that should NOT affect the breaker, call neither.
///
/// Thread-safe via `Mutex` (critical section is sub-microsecond, no async inside).
pub struct CircuitBreaker {
    state: Mutex<BreakerState>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        let state = BreakerState::new(config.window_size);
        Self {
            state: Mutex::new(state),
            config,
        }
    }

    pub fn status(&self) -> BreakerStatus {
        self.state.lock().unwrap().status
    }

    /// Check whether a call should be allowed. If the circuit is open and
    /// cooldown has elapsed, transitions to half-open and allows a probe.
    pub fn check(&self) -> Result<(), CircuitOpenError> {
        let mut state = self.state.lock().unwrap();
        match state.status {
            BreakerStatus::Closed | BreakerStatus::HalfOpen => Ok(()),
            BreakerStatus::Open => {
                if let Some(opened_at) = state.opened_at {
                    if opened_at.elapsed() >= self.config.cooldown {
                        state.status = BreakerStatus::HalfOpen;
                        tracing::info!("Circuit breaker transitioning to half-open");
                        Ok(())
                    } else {
                        Err(CircuitOpenError)
                    }
                } else {
                    state.status = BreakerStatus::HalfOpen;
                    Ok(())
                }
            }
        }
    }

    /// Record a successful call. Closes the breaker if in half-open state.
    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();
        self.push_result(&mut state, true);
        if state.status == BreakerStatus::HalfOpen {
            state.status = BreakerStatus::Closed;
            state.opened_at = None;
            tracing::info!("Circuit breaker closed after successful probe");
        }
    }

    /// Record a retryable (infrastructure) failure. May trip the breaker.
    pub fn record_retryable_failure(&self) {
        let mut state = self.state.lock().unwrap();
        self.push_result(&mut state, false);
        match state.status {
            BreakerStatus::HalfOpen => {
                state.status = BreakerStatus::Open;
                state.opened_at = Some(Instant::now());
                tracing::warn!("Circuit breaker re-opened after half-open probe failure");
            }
            BreakerStatus::Closed => {
                if self.should_trip(&state) {
                    state.status = BreakerStatus::Open;
                    state.opened_at = Some(Instant::now());
                    tracing::warn!("Circuit breaker tripped — too many retryable failures");
                }
            }
            BreakerStatus::Open => {}
        }
    }

    fn push_result(&self, state: &mut BreakerState, success: bool) {
        if state.window.len() >= self.config.window_size {
            state.window.pop_front();
        }
        state.window.push_back(success);
    }

    fn should_trip(&self, state: &BreakerState) -> bool {
        if state.window.len() < self.config.window_size {
            return false;
        }
        let failures = state.window.iter().filter(|&&s| !s).count();
        let ratio = failures as f64 / state.window.len() as f64;
        ratio >= self.config.failure_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            window_size: 4,
            failure_threshold: 0.5,
            cooldown: Duration::from_secs(5),
        }
    }

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new(small_config());
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[test]
    fn successes_stay_closed() {
        let cb = CircuitBreaker::new(small_config());
        for _ in 0..10 {
            assert!(cb.check().is_ok());
            cb.record_success();
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[test]
    fn trips_after_threshold() {
        let cb = CircuitBreaker::new(small_config());
        // 4 consecutive failures → 100% ≥ 50%
        for _ in 0..3 {
            cb.check().unwrap();
            cb.record_retryable_failure();
            assert_eq!(cb.status(), BreakerStatus::Closed);
        }
        cb.check().unwrap();
        cb.record_retryable_failure();
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[test]
    fn open_circuit_rejects() {
        let cb = CircuitBreaker::new(small_config());
        for _ in 0..4 {
            cb.check().unwrap();
            cb.record_retryable_failure();
        }
        assert!(cb.check().is_err());
    }

    #[test]
    fn below_threshold_stays_closed() {
        let cb = CircuitBreaker::new(small_config());
        // 1 fail + 3 success = 25% < 50%
        cb.check().unwrap();
        cb.record_retryable_failure();
        for _ in 0..3 {
            cb.check().unwrap();
            cb.record_success();
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[test]
    fn at_threshold_trips() {
        let cb = CircuitBreaker::new(small_config());
        // [success, success, fail, fail] → 50% ≥ 50%, trips on last failure
        cb.check().unwrap();
        cb.record_success();
        cb.check().unwrap();
        cb.record_success();
        cb.check().unwrap();
        cb.record_retryable_failure();
        cb.check().unwrap();
        cb.record_retryable_failure();
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[test]
    fn sliding_window_evicts_old_failures() {
        let cb = CircuitBreaker::new(small_config());
        // 3 success + 1 fail = 25% → closed. Then more success pushes fail out.
        cb.check().unwrap();
        cb.record_success();
        cb.check().unwrap();
        cb.record_success();
        cb.check().unwrap();
        cb.record_success();
        cb.check().unwrap();
        cb.record_retryable_failure(); // window: [s, s, s, f] = 25%
        assert_eq!(cb.status(), BreakerStatus::Closed);

        // 4 more successes → window becomes [s, s, s, s]
        for _ in 0..4 {
            cb.check().unwrap();
            cb.record_success();
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn transitions_to_half_open_after_cooldown() {
        let cb = CircuitBreaker::new(small_config());
        for _ in 0..4 {
            cb.check().unwrap();
            cb.record_retryable_failure();
        }
        assert_eq!(cb.status(), BreakerStatus::Open);

        tokio::time::advance(Duration::from_secs(6)).await;

        assert!(cb.check().is_ok());
        assert_eq!(cb.status(), BreakerStatus::HalfOpen);
    }

    #[tokio::test(start_paused = true)]
    async fn successful_probe_closes() {
        let cb = CircuitBreaker::new(small_config());
        for _ in 0..4 {
            cb.check().unwrap();
            cb.record_retryable_failure();
        }
        tokio::time::advance(Duration::from_secs(6)).await;

        cb.check().unwrap();
        cb.record_success();
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_probe_reopens() {
        let cb = CircuitBreaker::new(small_config());
        for _ in 0..4 {
            cb.check().unwrap();
            cb.record_retryable_failure();
        }
        tokio::time::advance(Duration::from_secs(6)).await;

        cb.check().unwrap();
        cb.record_retryable_failure();
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[test]
    fn unrecorded_calls_do_not_affect_state() {
        let cb = CircuitBreaker::new(small_config());
        // Simulate 10 business declines (no record_* calls)
        for _ in 0..10 {
            cb.check().unwrap();
            // caller doesn't call record_success or record_retryable_failure
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }
}
