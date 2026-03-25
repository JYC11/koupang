use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use shared::circuit_breaker::{BreakerStatus, CircuitBreaker, CircuitBreakerConfig};
use uuid::Uuid;

use super::traits::{
    GatewayAuthResult, GatewayCaptureResult, GatewayError, GatewayRefundResult, GatewayVoidResult,
    PaymentGateway,
};

/// Circuit breaker decorator for `PaymentGateway`. Delegates state management
/// to `shared::circuit_breaker::CircuitBreaker`; only retryable gateway errors
/// trip the breaker — business declines pass through unrecorded.
pub struct CircuitBreakerGateway {
    inner: Arc<dyn PaymentGateway>,
    breaker: CircuitBreaker,
}

impl CircuitBreakerGateway {
    pub fn new(inner: Arc<dyn PaymentGateway>, config: CircuitBreakerConfig) -> Self {
        Self {
            inner,
            breaker: CircuitBreaker::new(config),
        }
    }

    pub fn status(&self) -> BreakerStatus {
        self.breaker.status()
    }

    fn pre_call(&self) -> Result<(), GatewayError> {
        self.breaker.check().map_err(|_| GatewayError {
            code: "CIRCUIT_OPEN".to_string(),
            message: "Circuit breaker is open — gateway temporarily unavailable".to_string(),
            reason: None,
            is_retryable: true,
        })
    }

    fn record_outcome(&self, err: Option<&GatewayError>) {
        match err {
            None => self.breaker.record_success(),
            Some(e) if e.is_retryable => self.breaker.record_retryable_failure(),
            Some(_) => {} // Business decline — don't record.
        }
    }
}

#[async_trait]
impl PaymentGateway for CircuitBreakerGateway {
    async fn authorize(
        &self,
        idempotency_key: &str,
        order_id: Uuid,
        amount: Decimal,
        currency: &str,
    ) -> Result<GatewayAuthResult, GatewayError> {
        self.pre_call()?;
        let result = self
            .inner
            .authorize(idempotency_key, order_id, amount, currency)
            .await;
        self.record_outcome(result.as_ref().err());
        result
    }

    async fn capture(&self, gateway_reference: &str) -> Result<GatewayCaptureResult, GatewayError> {
        self.pre_call()?;
        let result = self.inner.capture(gateway_reference).await;
        self.record_outcome(result.as_ref().err());
        result
    }

    async fn void(&self, gateway_reference: &str) -> Result<GatewayVoidResult, GatewayError> {
        self.pre_call()?;
        let result = self.inner.void(gateway_reference).await;
        self.record_outcome(result.as_ref().err());
        result
    }

    async fn refund(
        &self,
        gateway_reference: &str,
        amount: Decimal,
    ) -> Result<GatewayRefundResult, GatewayError> {
        self.pre_call()?;
        let result = self.inner.refund(gateway_reference, amount).await;
        self.record_outcome(result.as_ref().err());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::mock::MockPaymentGateway;
    use crate::gateway::traits::GatewayStatus;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Gateway that plays back a scripted sequence of outcomes.
    /// `Some(true)` = success, `Some(false)` = retryable failure, `None` = business decline.
    struct ScriptedGateway {
        script: Vec<Option<bool>>,
        call_count: AtomicUsize,
    }

    impl ScriptedGateway {
        fn new(script: Vec<Option<bool>>) -> Self {
            Self {
                script,
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl PaymentGateway for ScriptedGateway {
        async fn authorize(
            &self,
            _key: &str,
            _order_id: Uuid,
            amount: Decimal,
            _currency: &str,
        ) -> Result<GatewayAuthResult, GatewayError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            match self.script.get(idx).copied().flatten() {
                Some(true) => Ok(GatewayAuthResult {
                    gateway_reference: format!("ref-{idx}"),
                    approved_amount: amount,
                    status: GatewayStatus::Success,
                }),
                Some(false) => Err(GatewayError::timeout("scripted timeout")),
                None => Err(GatewayError::declined("scripted decline")),
            }
        }

        async fn capture(&self, _ref: &str) -> Result<GatewayCaptureResult, GatewayError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            match self.script.get(idx).copied().flatten() {
                Some(true) => Ok(GatewayCaptureResult {
                    status: GatewayStatus::Success,
                }),
                Some(false) => Err(GatewayError::timeout("scripted timeout")),
                None => Err(GatewayError::declined("scripted decline")),
            }
        }

        async fn void(&self, _ref: &str) -> Result<GatewayVoidResult, GatewayError> {
            Ok(GatewayVoidResult {
                status: GatewayStatus::Success,
            })
        }

        async fn refund(
            &self,
            _ref: &str,
            _amount: Decimal,
        ) -> Result<GatewayRefundResult, GatewayError> {
            Ok(GatewayRefundResult {
                status: GatewayStatus::Success,
            })
        }
    }

    fn authorize(
        gw: &CircuitBreakerGateway,
    ) -> impl std::future::Future<Output = Result<GatewayAuthResult, GatewayError>> + '_ {
        gw.authorize("key", Uuid::now_v7(), Decimal::new(1000, 2), "USD")
    }

    fn small_window_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            window_size: 4,
            failure_threshold: 0.5,
            cooldown: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn starts_closed() {
        let inner = Arc::new(MockPaymentGateway::always_succeeds());
        let cb = CircuitBreakerGateway::new(inner, small_window_config());
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test]
    async fn success_calls_stay_closed() {
        let inner = Arc::new(MockPaymentGateway::always_succeeds());
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..10 {
            assert!(authorize(&cb).await.is_ok());
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test]
    async fn trips_after_threshold_retryable_failures() {
        let script = vec![Some(false), Some(false), Some(false), Some(false)];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..3 {
            let _ = authorize(&cb).await;
            assert_eq!(cb.status(), BreakerStatus::Closed);
        }

        let _ = authorize(&cb).await;
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[tokio::test]
    async fn business_declines_do_not_trip_breaker() {
        let script = vec![None, None, None, None, None, None];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..6 {
            let _ = authorize(&cb).await;
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test]
    async fn open_circuit_returns_circuit_open_error() {
        let script = vec![Some(false), Some(false), Some(false), Some(false)];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..4 {
            let _ = authorize(&cb).await;
        }

        let err = authorize(&cb).await.unwrap_err();
        assert_eq!(err.code, "CIRCUIT_OPEN");
        assert!(err.is_retryable);
    }

    #[tokio::test(start_paused = true)]
    async fn transitions_to_half_open_after_cooldown() {
        let script = vec![
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(true), // probe after cooldown
        ];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..4 {
            let _ = authorize(&cb).await;
        }
        assert_eq!(cb.status(), BreakerStatus::Open);

        tokio::time::advance(Duration::from_secs(6)).await;

        assert!(authorize(&cb).await.is_ok());
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_probe_reopens_breaker() {
        let script = vec![
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false), // probe fails
        ];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..4 {
            let _ = authorize(&cb).await;
        }

        tokio::time::advance(Duration::from_secs(6)).await;

        let _ = authorize(&cb).await;
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[tokio::test]
    async fn mixed_results_below_threshold_stay_closed() {
        let script = vec![Some(false), Some(true), Some(true), Some(true)];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..4 {
            let _ = authorize(&cb).await;
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }

    #[tokio::test]
    async fn mixed_results_at_threshold_trips() {
        // Threshold checked on failure, so tripping call must be a failure.
        let script = vec![Some(true), Some(true), Some(false), Some(false)];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..4 {
            let _ = authorize(&cb).await;
        }
        assert_eq!(cb.status(), BreakerStatus::Open);
    }

    #[tokio::test]
    async fn sliding_window_evicts_old_results() {
        let script: Vec<Option<bool>> = vec![
            Some(true),
            Some(true),
            Some(true),
            Some(false), // window: [t, t, t, f] = 25%
            Some(true),  // window: [t, t, f, t] = 25%
            Some(true),  // window: [t, f, t, t] = 25%
            Some(true),  // window: [f, t, t, t] = 25%
            Some(true),  // window: [t, t, t, t] = 0%
        ];
        let inner = Arc::new(ScriptedGateway::new(script));
        let cb = CircuitBreakerGateway::new(inner, small_window_config());

        for _ in 0..8 {
            let _ = authorize(&cb).await;
        }
        assert_eq!(cb.status(), BreakerStatus::Closed);
    }
}
