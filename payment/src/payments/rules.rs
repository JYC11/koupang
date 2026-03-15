use crate::ledger::value_objects::PaymentState;
use rust_decimal::Decimal;
use shared::rules::Rule;
use std::fmt;

// ── Check enum ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PaymentCheck {
    MinAmount(Decimal),
    MaxAmount(Decimal),
    SupportedCurrency,
    ValidStateForOperation,
    CaptureAmountConsistent,
}

impl fmt::Display for PaymentCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MinAmount(v) => write!(f, "amount must be at least ${v}"),
            Self::MaxAmount(v) => write!(f, "amount must not exceed ${v}"),
            Self::SupportedCurrency => write!(f, "currency must be supported"),
            Self::ValidStateForOperation => {
                write!(f, "payment must be in valid state for operation")
            }
            Self::CaptureAmountConsistent => {
                write!(f, "capture amount must match authorization")
            }
        }
    }
}

// ── Thresholds ───────────────────────────────────────────

pub const MIN_PAYMENT_AMOUNT: Decimal = Decimal::from_parts(50, 0, 0, false, 2); // $0.50
pub const MAX_PAYMENT_AMOUNT: Decimal = Decimal::from_parts(100_000, 0, 0, false, 0);
pub const SUPPORTED_CURRENCIES: &[&str] = &["USD", "KRW", "EUR", "GBP", "JPY"];

// ── Rule trees ───────────────────────────────────────────

pub fn authorization_rules() -> Rule<PaymentCheck> {
    Rule::all(vec![
        Rule::check(PaymentCheck::MinAmount(MIN_PAYMENT_AMOUNT)),
        Rule::check(PaymentCheck::MaxAmount(MAX_PAYMENT_AMOUNT)),
        Rule::check(PaymentCheck::SupportedCurrency),
        Rule::check(PaymentCheck::ValidStateForOperation),
    ])
}

pub fn capture_rules() -> Rule<PaymentCheck> {
    Rule::all(vec![
        Rule::check(PaymentCheck::ValidStateForOperation),
        Rule::check(PaymentCheck::CaptureAmountConsistent),
    ])
}

// ── Contexts ─────────────────────────────────────────────

#[derive(Debug)]
pub struct AuthorizationContext {
    pub amount: Decimal,
    pub currency: String,
    pub payment_state: PaymentState,
}

#[derive(Debug)]
pub struct CaptureContext {
    pub payment_state: PaymentState,
    pub auth_amount: Decimal,
    pub capture_amount: Decimal,
}

// ── Predicates ───────────────────────────────────────────

pub fn eval_authorization(ctx: &AuthorizationContext) -> impl Fn(&PaymentCheck) -> bool + '_ {
    move |check| match check {
        PaymentCheck::MinAmount(min) => ctx.amount >= *min,
        PaymentCheck::MaxAmount(max) => ctx.amount <= *max,
        PaymentCheck::SupportedCurrency => SUPPORTED_CURRENCIES.contains(&ctx.currency.as_str()),
        PaymentCheck::ValidStateForOperation => {
            matches!(ctx.payment_state, PaymentState::New | PaymentState::Failed)
        }
        PaymentCheck::CaptureAmountConsistent => true, // Not applicable
    }
}

pub fn eval_capture(ctx: &CaptureContext) -> impl Fn(&PaymentCheck) -> bool + '_ {
    move |check| match check {
        PaymentCheck::ValidStateForOperation => {
            matches!(ctx.payment_state, PaymentState::Authorized)
        }
        PaymentCheck::CaptureAmountConsistent => ctx.capture_amount == ctx.auth_amount,
        _ => true, // Not applicable for capture
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_auth_ctx() -> AuthorizationContext {
        AuthorizationContext {
            amount: Decimal::new(5000, 2), // $50.00
            currency: "USD".to_string(),
            payment_state: PaymentState::New,
        }
    }

    #[test]
    fn authorization_rules_pass_for_valid_payment() {
        let rules = authorization_rules();
        let ctx = valid_auth_ctx();
        assert!(rules.evaluate(&eval_authorization(&ctx)));
    }

    #[test]
    fn authorization_fails_below_min() {
        let rules = authorization_rules();
        let ctx = AuthorizationContext {
            amount: Decimal::new(10, 2), // $0.10
            ..valid_auth_ctx()
        };
        let result = rules.evaluate_detailed(&eval_authorization(&ctx));
        assert!(!result.passed());
        assert!(
            result
                .failure_messages()
                .iter()
                .any(|m| m.contains("at least"))
        );
    }

    #[test]
    fn authorization_fails_above_max() {
        let rules = authorization_rules();
        let ctx = AuthorizationContext {
            amount: Decimal::new(200_000, 0),
            ..valid_auth_ctx()
        };
        let result = rules.evaluate_detailed(&eval_authorization(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn authorization_fails_unsupported_currency() {
        let rules = authorization_rules();
        let ctx = AuthorizationContext {
            currency: "XYZ".to_string(),
            ..valid_auth_ctx()
        };
        let result = rules.evaluate_detailed(&eval_authorization(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn authorization_fails_wrong_state() {
        let rules = authorization_rules();
        let ctx = AuthorizationContext {
            payment_state: PaymentState::Captured,
            ..valid_auth_ctx()
        };
        let result = rules.evaluate_detailed(&eval_authorization(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn authorization_passes_from_failed_state() {
        let rules = authorization_rules();
        let ctx = AuthorizationContext {
            payment_state: PaymentState::Failed,
            ..valid_auth_ctx()
        };
        assert!(rules.evaluate(&eval_authorization(&ctx)));
    }

    #[test]
    fn capture_rules_pass_for_authorized() {
        let rules = capture_rules();
        let ctx = CaptureContext {
            payment_state: PaymentState::Authorized,
            auth_amount: Decimal::new(5000, 2),
            capture_amount: Decimal::new(5000, 2),
        };
        assert!(rules.evaluate(&eval_capture(&ctx)));
    }

    #[test]
    fn capture_fails_wrong_state() {
        let rules = capture_rules();
        let ctx = CaptureContext {
            payment_state: PaymentState::New,
            auth_amount: Decimal::new(5000, 2),
            capture_amount: Decimal::new(5000, 2),
        };
        let result = rules.evaluate_detailed(&eval_capture(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn capture_fails_amount_mismatch() {
        let rules = capture_rules();
        let ctx = CaptureContext {
            payment_state: PaymentState::Authorized,
            auth_amount: Decimal::new(5000, 2),
            capture_amount: Decimal::new(3000, 2),
        };
        let result = rules.evaluate_detailed(&eval_capture(&ctx));
        assert!(!result.passed());
        assert!(
            result
                .failure_messages()
                .iter()
                .any(|m| m.contains("match"))
        );
    }

    #[test]
    fn describe_authorization_rules() {
        let rules = authorization_rules();
        let desc = rules.describe();
        assert!(desc.contains("amount must be at least"));
        assert!(desc.contains("AND"));
    }

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;

        fn arb_payment_state() -> impl Strategy<Value = PaymentState> {
            prop_oneof![
                Just(PaymentState::New),
                Just(PaymentState::Authorized),
                Just(PaymentState::Captured),
                Just(PaymentState::Voided),
                Just(PaymentState::Refunded),
                Just(PaymentState::Pending),
                Just(PaymentState::Failed),
            ]
        }

        fn arb_auth_ctx() -> impl Strategy<Value = AuthorizationContext> {
            (
                (0i64..20_000_000).prop_map(|c| Decimal::new(c, 2)),
                prop_oneof![
                    Just("USD".to_string()),
                    Just("KRW".to_string()),
                    Just("XYZ".to_string()),
                ],
                arb_payment_state(),
            )
                .prop_map(|(amount, currency, state)| AuthorizationContext {
                    amount,
                    currency,
                    payment_state: state,
                })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(500))]

            // evaluate and evaluate_detailed agree for authorization rules.
            #[test]
            fn auth_eval_agrees_with_detailed(ctx in arb_auth_ctx()) {
                let rules = authorization_rules();
                let pred = eval_authorization(&ctx);
                prop_assert_eq!(rules.evaluate(&pred), rules.evaluate_detailed(&pred).passed());
            }

            // Only New and Failed allow authorization.
            #[test]
            fn auth_only_from_new_or_failed(state in arb_payment_state()) {
                let ctx = AuthorizationContext {
                    amount: Decimal::new(5000, 2),
                    currency: "USD".to_string(),
                    payment_state: state,
                };
                let pred = eval_authorization(&ctx);
                let valid_state = matches!(state, PaymentState::New | PaymentState::Failed);
                // If state is invalid, the ValidStateForOperation check must fail.
                let result = authorization_rules().evaluate_detailed(&pred);
                if !valid_state {
                    prop_assert!(!result.passed());
                }
            }

            // Capture amount must match auth amount.
            #[test]
            fn capture_detects_amount_mismatch(
                auth in (1i64..100_000).prop_map(|c| Decimal::new(c, 2)),
                delta in (1i64..1000).prop_map(|c| Decimal::new(c, 2)),
            ) {
                let ctx = CaptureContext {
                    payment_state: PaymentState::Authorized,
                    auth_amount: auth,
                    capture_amount: auth + delta,
                };
                let result = capture_rules().evaluate_detailed(&eval_capture(&ctx));
                prop_assert!(!result.passed());
            }

            // Capture with matching amount and Authorized state always passes.
            #[test]
            fn capture_passes_when_valid(amount in (1i64..100_000).prop_map(|c| Decimal::new(c, 2))) {
                let ctx = CaptureContext {
                    payment_state: PaymentState::Authorized,
                    auth_amount: amount,
                    capture_amount: amount,
                };
                prop_assert!(capture_rules().evaluate(&eval_capture(&ctx)));
            }
        }
    }
}
