use rust_decimal::Decimal;
use shared::rules::Rule;
use std::fmt;

use crate::cart::domain::Cart;

// ── Check enum ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CheckoutCheck {
    CartNotEmpty,
    MinCartValue(Decimal),
    MaxCartValue(Decimal),
    AllItemsHaveValidPrices,
    MaxCartItems(usize),
    AllQuantitiesWithinLimits,
}

impl fmt::Display for CheckoutCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CartNotEmpty => write!(f, "cart must not be empty"),
            Self::MinCartValue(v) => write!(f, "cart total must be at least ${v}"),
            Self::MaxCartValue(v) => write!(f, "cart total must not exceed ${v}"),
            Self::AllItemsHaveValidPrices => write!(f, "all items must have valid prices"),
            Self::MaxCartItems(n) => write!(f, "cart must not exceed {n} items"),
            Self::AllQuantitiesWithinLimits => write!(f, "all quantities must be within limits"),
        }
    }
}

// ── Thresholds ───────────────────────────────────────────

pub const MIN_CART_VALUE: Decimal = Decimal::ONE;
pub const MAX_CART_VALUE: Decimal = Decimal::from_parts(50_000, 0, 0, false, 0);
pub const MAX_CART_ITEMS: usize = 50;

// ── Rule tree ────────────────────────────────────────────

pub fn checkout_readiness_rules() -> Rule<CheckoutCheck> {
    Rule::all(vec![
        Rule::check(CheckoutCheck::CartNotEmpty),
        Rule::check(CheckoutCheck::MaxCartItems(MAX_CART_ITEMS)),
        Rule::check(CheckoutCheck::MinCartValue(MIN_CART_VALUE)),
        Rule::check(CheckoutCheck::MaxCartValue(MAX_CART_VALUE)),
        Rule::check(CheckoutCheck::AllItemsHaveValidPrices),
        Rule::check(CheckoutCheck::AllQuantitiesWithinLimits),
    ])
}

// ── Context ──────────────────────────────────────────────

#[derive(Debug)]
pub struct CheckoutContext {
    pub item_count: usize,
    pub cart_total: Decimal,
    pub all_prices_valid: bool,
    pub all_quantities_valid: bool,
}

impl CheckoutContext {
    pub fn from_cart(cart: &Cart) -> Self {
        Self {
            item_count: cart.item_count(),
            cart_total: cart.total(),
            all_prices_valid: cart
                .items
                .iter()
                .all(|i| i.unit_price.value() > Decimal::ZERO),
            all_quantities_valid: cart
                .items
                .iter()
                .all(|i| i.quantity.value() >= 1 && i.quantity.value() <= 99),
        }
    }
}

// ── Predicate ────────────────────────────────────────────

pub fn eval_checkout(ctx: &CheckoutContext) -> impl Fn(&CheckoutCheck) -> bool + '_ {
    move |check| match check {
        CheckoutCheck::CartNotEmpty => ctx.item_count > 0,
        CheckoutCheck::MinCartValue(min) => ctx.cart_total >= *min,
        CheckoutCheck::MaxCartValue(max) => ctx.cart_total <= *max,
        CheckoutCheck::AllItemsHaveValidPrices => ctx.all_prices_valid,
        CheckoutCheck::MaxCartItems(max) => ctx.item_count <= *max,
        CheckoutCheck::AllQuantitiesWithinLimits => ctx.all_quantities_valid,
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_ctx() -> CheckoutContext {
        CheckoutContext {
            item_count: 3,
            cart_total: Decimal::new(5000, 2), // $50.00
            all_prices_valid: true,
            all_quantities_valid: true,
        }
    }

    #[test]
    fn checkout_rules_pass_for_valid_cart() {
        let rules = checkout_readiness_rules();
        let ctx = passing_ctx();
        assert!(rules.evaluate(&eval_checkout(&ctx)));
    }

    #[test]
    fn checkout_fails_for_empty_cart() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            item_count: 0,
            cart_total: Decimal::ZERO,
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
        let msgs = result.failure_messages();
        assert!(msgs.iter().any(|m| m.contains("empty")));
    }

    #[test]
    fn checkout_fails_below_min_value() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            cart_total: Decimal::new(50, 2), // $0.50
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
        let msgs = result.failure_messages();
        assert!(msgs.iter().any(|m| m.contains("at least")));
    }

    #[test]
    fn checkout_fails_above_max_value() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            cart_total: Decimal::new(6_000_000, 2), // $60,000
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
        let msgs = result.failure_messages();
        assert!(msgs.iter().any(|m| m.contains("exceed")));
    }

    #[test]
    fn checkout_fails_too_many_items() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            item_count: 51,
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn checkout_fails_invalid_prices() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            all_prices_valid: false,
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn checkout_fails_invalid_quantities() {
        let rules = checkout_readiness_rules();
        let ctx = CheckoutContext {
            all_quantities_valid: false,
            ..passing_ctx()
        };
        let result = rules.evaluate_detailed(&eval_checkout(&ctx));
        assert!(!result.passed());
    }

    #[test]
    fn describe_produces_readable_output() {
        let rules = checkout_readiness_rules();
        let desc = rules.describe();
        assert!(desc.contains("cart must not be empty"));
        assert!(desc.contains("AND"));
    }

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;

        fn arb_checkout_ctx() -> impl Strategy<Value = CheckoutContext> {
            (
                0usize..100,
                (0i64..10_000_000).prop_map(|c| Decimal::new(c, 2)),
                any::<bool>(),
                any::<bool>(),
            )
                .prop_map(|(items, total, prices, quantities)| CheckoutContext {
                    item_count: items,
                    cart_total: total,
                    all_prices_valid: prices,
                    all_quantities_valid: quantities,
                })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(500))]

            // evaluate and evaluate_detailed agree.
            #[test]
            fn checkout_eval_agrees_with_detailed(ctx in arb_checkout_ctx()) {
                let rules = checkout_readiness_rules();
                let pred = eval_checkout(&ctx);
                prop_assert_eq!(rules.evaluate(&pred), rules.evaluate_detailed(&pred).passed());
            }

            // Empty cart always fails checkout.
            #[test]
            fn empty_cart_always_fails(
                total in (0i64..10_000_000).prop_map(|c| Decimal::new(c, 2)),
            ) {
                let ctx = CheckoutContext {
                    item_count: 0,
                    cart_total: total,
                    all_prices_valid: true,
                    all_quantities_valid: true,
                };
                prop_assert!(!checkout_readiness_rules().evaluate(&eval_checkout(&ctx)));
            }

            // Valid cart with reasonable values always passes.
            #[test]
            fn valid_cart_passes(
                items in 1usize..=50,
                total in (100i64..4_000_000).prop_map(|c| Decimal::new(c, 2)),
            ) {
                let ctx = CheckoutContext {
                    item_count: items,
                    cart_total: total,
                    all_prices_valid: true,
                    all_quantities_valid: true,
                };
                prop_assert!(checkout_readiness_rules().evaluate(&eval_checkout(&ctx)));
            }

            // Cart over max items fails.
            #[test]
            fn over_max_items_fails(items in 51usize..200) {
                let ctx = CheckoutContext {
                    item_count: items,
                    cart_total: Decimal::new(5000, 2),
                    all_prices_valid: true,
                    all_quantities_valid: true,
                };
                prop_assert!(!checkout_readiness_rules().evaluate(&eval_checkout(&ctx)));
            }
        }
    }
}
