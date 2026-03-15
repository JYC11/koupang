use crate::orders::value_objects::OrderStatus;
use rust_decimal::Decimal;
use shared::rules::Rule;
use std::fmt;

// ── Check enum ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum OrderCheck {
    NonEmptyItems,
    MinOrderAmount(Decimal),
    MaxItemsPerOrder(usize),
    SupportedCurrency,
    ShippingAddressComplete,
    PositiveItemPrices,
    StatusAllowsCancellation,
}

impl fmt::Display for OrderCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonEmptyItems => write!(f, "order must have at least one item"),
            Self::MinOrderAmount(v) => write!(f, "order total must be at least ${v}"),
            Self::MaxItemsPerOrder(n) => write!(f, "order must not exceed {n} items"),
            Self::SupportedCurrency => write!(f, "currency must be supported"),
            Self::ShippingAddressComplete => write!(f, "shipping address must be complete"),
            Self::PositiveItemPrices => write!(f, "all item prices must be positive"),
            Self::StatusAllowsCancellation => write!(f, "order status must allow cancellation"),
        }
    }
}

// ── Thresholds ───────────────────────────────────────────

pub const MIN_ORDER_AMOUNT: Decimal = Decimal::ONE;
pub const MAX_ITEMS_PER_ORDER: usize = 100;
pub const SUPPORTED_CURRENCIES: &[&str] = &["USD", "KRW", "EUR", "GBP", "JPY"];

// ── Rule trees ───────────────────────────────────────────

pub fn creation_rules() -> Rule<OrderCheck> {
    Rule::all(vec![
        Rule::check(OrderCheck::NonEmptyItems),
        Rule::check(OrderCheck::MaxItemsPerOrder(MAX_ITEMS_PER_ORDER)),
        Rule::check(OrderCheck::MinOrderAmount(MIN_ORDER_AMOUNT)),
        Rule::check(OrderCheck::SupportedCurrency),
        Rule::check(OrderCheck::ShippingAddressComplete),
        Rule::check(OrderCheck::PositiveItemPrices),
    ])
}

pub fn cancellation_rules() -> Rule<OrderCheck> {
    Rule::all(vec![Rule::check(OrderCheck::StatusAllowsCancellation)])
}

// ── Contexts ─────────────────────────────────────────────

#[derive(Debug)]
pub struct CreateOrderContext {
    pub item_count: usize,
    pub total_amount: Decimal,
    pub currency: String,
    pub shipping_complete: bool,
    pub all_prices_positive: bool,
}

#[derive(Debug)]
pub struct CancellationContext {
    pub current_status: OrderStatus,
}

// ── Predicates ───────────────────────────────────────────

pub fn eval_creation(ctx: &CreateOrderContext) -> impl Fn(&OrderCheck) -> bool + '_ {
    move |check| match check {
        OrderCheck::NonEmptyItems => ctx.item_count > 0,
        OrderCheck::MinOrderAmount(min) => ctx.total_amount >= *min,
        OrderCheck::MaxItemsPerOrder(max) => ctx.item_count <= *max,
        OrderCheck::SupportedCurrency => SUPPORTED_CURRENCIES.contains(&ctx.currency.as_str()),
        OrderCheck::ShippingAddressComplete => ctx.shipping_complete,
        OrderCheck::PositiveItemPrices => ctx.all_prices_positive,
        OrderCheck::StatusAllowsCancellation => false, // Not applicable for creation
    }
}

pub fn eval_cancellation(ctx: &CancellationContext) -> impl Fn(&OrderCheck) -> bool + '_ {
    move |check| match check {
        OrderCheck::StatusAllowsCancellation => ctx.current_status.can_cancel(),
        _ => true, // Non-cancellation checks pass by default
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_creation_ctx() -> CreateOrderContext {
        CreateOrderContext {
            item_count: 2,
            total_amount: Decimal::new(5000, 2),
            currency: "USD".to_string(),
            shipping_complete: true,
            all_prices_positive: true,
        }
    }

    #[test]
    fn creation_rules_pass_for_valid_order() {
        let rules = creation_rules();
        let ctx = valid_creation_ctx();
        assert!(rules.evaluate(&eval_creation(&ctx)));
    }

    #[test]
    fn creation_fails_empty_items() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            item_count: 0,
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
        assert!(
            result
                .failure_messages()
                .iter()
                .any(|m| m.contains("at least one"))
        );
    }

    #[test]
    fn creation_fails_below_min_amount() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            total_amount: Decimal::new(50, 2), // $0.50
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn creation_fails_too_many_items() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            item_count: 101,
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn creation_fails_unsupported_currency() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            currency: "XYZ".to_string(),
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
        assert!(
            result
                .failure_messages()
                .iter()
                .any(|m| m.contains("currency"))
        );
    }

    #[test]
    fn creation_fails_incomplete_address() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            shipping_complete: false,
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn creation_fails_zero_prices() {
        let rules = creation_rules();
        let ctx = CreateOrderContext {
            all_prices_positive: false,
            ..valid_creation_ctx()
        };
        let result = rules.evaluate_detailed(&eval_creation(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn cancellation_passes_from_pending() {
        let rules = cancellation_rules();
        let ctx = CancellationContext {
            current_status: OrderStatus::Pending,
        };
        assert!(rules.evaluate(&eval_cancellation(&ctx)));
    }

    #[test]
    fn cancellation_fails_from_cancelled() {
        let rules = cancellation_rules();
        let ctx = CancellationContext {
            current_status: OrderStatus::Cancelled,
        };
        let result = rules.evaluate_detailed(&eval_cancellation(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn cancellation_fails_from_delivered() {
        let rules = cancellation_rules();
        let ctx = CancellationContext {
            current_status: OrderStatus::Delivered,
        };
        assert!(!rules.evaluate(&eval_cancellation(&ctx)));
    }

    #[test]
    fn describe_creation_rules() {
        let rules = creation_rules();
        let desc = rules.describe();
        assert!(desc.contains("order must have at least one item"));
        assert!(desc.contains("AND"));
    }

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;

        fn arb_creation_ctx() -> impl Strategy<Value = CreateOrderContext> {
            (
                0usize..200,
                (0i64..10_000_000).prop_map(|c| Decimal::new(c, 2)),
                prop_oneof![
                    Just("USD".to_string()),
                    Just("KRW".to_string()),
                    Just("EUR".to_string()),
                    Just("XYZ".to_string()),
                    Just("".to_string()),
                ],
                any::<bool>(),
                any::<bool>(),
            )
                .prop_map(|(item_count, total_amount, currency, shipping, prices)| {
                    CreateOrderContext {
                        item_count,
                        total_amount,
                        currency,
                        shipping_complete: shipping,
                        all_prices_positive: prices,
                    }
                })
        }

        fn arb_order_status() -> impl Strategy<Value = OrderStatus> {
            prop_oneof![
                Just(OrderStatus::Pending),
                Just(OrderStatus::InventoryReserved),
                Just(OrderStatus::PaymentAuthorized),
                Just(OrderStatus::Confirmed),
                Just(OrderStatus::Shipped),
                Just(OrderStatus::Delivered),
                Just(OrderStatus::Cancelled),
                Just(OrderStatus::Returned),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(500))]

            // evaluate and evaluate_detailed agree for creation rules.
            #[test]
            fn creation_eval_agrees_with_detailed(ctx in arb_creation_ctx()) {
                let rules = creation_rules();
                let pred = eval_creation(&ctx);
                prop_assert_eq!(rules.evaluate(&pred), rules.evaluate_detailed(&pred).passed());
            }

            // evaluate and evaluate_detailed agree for cancellation rules.
            #[test]
            fn cancellation_eval_agrees_with_detailed(status in arb_order_status()) {
                let ctx = CancellationContext { current_status: status };
                let rules = cancellation_rules();
                let pred = eval_cancellation(&ctx);
                prop_assert_eq!(rules.evaluate(&pred), rules.evaluate_detailed(&pred).passed());
            }

            // Terminal states never allow cancellation.
            #[test]
            fn terminal_states_reject_cancellation(status in prop_oneof![
                Just(OrderStatus::Cancelled),
                Just(OrderStatus::Returned),
                Just(OrderStatus::Delivered),
            ]) {
                let ctx = CancellationContext { current_status: status };
                prop_assert!(!cancellation_rules().evaluate(&eval_cancellation(&ctx)));
            }

            // Non-terminal states (except Delivered) allow cancellation.
            #[test]
            fn non_terminal_states_allow_cancellation(status in prop_oneof![
                Just(OrderStatus::Pending),
                Just(OrderStatus::InventoryReserved),
                Just(OrderStatus::PaymentAuthorized),
                Just(OrderStatus::Confirmed),
                Just(OrderStatus::Shipped),
            ]) {
                let ctx = CancellationContext { current_status: status };
                prop_assert!(cancellation_rules().evaluate(&eval_cancellation(&ctx)));
            }
        }
    }
}
