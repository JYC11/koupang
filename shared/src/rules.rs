//! Generic composable rule algebra — rules as data with multiple interpreters.
//!
//! Ported from the DOP (Data-Oriented Programming) pattern: represent validation
//! logic as an algebraic data type, then write interpreters that traverse it.
//!
//! # Example
//! ```
//! use shared::rules::Rule;
//!
//! #[derive(Debug, Clone)]
//! enum CartCheck {
//!     MaxItems(usize),
//!     MinTotal(f64),
//!     RequiresAuth,
//! }
//!
//! impl std::fmt::Display for CartCheck {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         match self {
//!             Self::MaxItems(n) => write!(f, "max {n} items"),
//!             Self::MinTotal(v) => write!(f, "minimum total ${v:.2}"),
//!             Self::RequiresAuth => write!(f, "requires authentication"),
//!         }
//!     }
//! }
//!
//! let checkout_rules = Rule::all(vec![
//!     Rule::check(CartCheck::RequiresAuth),
//!     Rule::check(CartCheck::MaxItems(50)),
//!     Rule::any(vec![
//!         Rule::check(CartCheck::MinTotal(1.0)),
//!     ]),
//! ]);
//!
//! assert_eq!(
//!     checkout_rules.describe(),
//!     "(requires authentication AND max 50 items AND (minimum total $1.00))"
//! );
//! ```

use std::fmt;

/// Composable rule tree. `A` is the leaf check type — domain-specific.
#[derive(Debug, Clone)]
pub enum Rule<A> {
    /// A single domain-specific check.
    Check(A),
    /// All sub-rules must hold.
    All(Vec<Rule<A>>),
    /// At least one sub-rule must hold.
    Any(Vec<Rule<A>>),
    /// Negation.
    Not(Box<Rule<A>>),
}

// ── Smart constructors ──────────────────────────────────────

impl<A> Rule<A> {
    pub fn check(value: A) -> Self {
        Self::Check(value)
    }

    pub fn all(rules: Vec<Self>) -> Self {
        Self::All(rules)
    }

    pub fn any(rules: Vec<Self>) -> Self {
        Self::Any(rules)
    }

    pub fn not(rule: Self) -> Self {
        Self::Not(Box::new(rule))
    }

    /// Fluent AND: combines self with another rule.
    pub fn and(self, other: Self) -> Self {
        match self {
            Self::All(mut rules) => {
                rules.push(other);
                Self::All(rules)
            }
            _ => Self::All(vec![self, other]),
        }
    }

    /// Fluent OR: combines self with another rule.
    pub fn or(self, other: Self) -> Self {
        match self {
            Self::Any(mut rules) => {
                rules.push(other);
                Self::Any(rules)
            }
            _ => Self::Any(vec![self, other]),
        }
    }
}

// ── Interpreter: evaluate ───────────────────────────────────

impl<A> Rule<A> {
    /// Evaluate the rule tree against a predicate on leaf checks.
    pub fn evaluate(&self, predicate: &impl Fn(&A) -> bool) -> bool {
        match self {
            Self::Check(a) => predicate(a),
            Self::All(rules) => rules.iter().all(|r| r.evaluate(predicate)),
            Self::Any(rules) => rules.iter().any(|r| r.evaluate(predicate)),
            Self::Not(r) => !r.evaluate(predicate),
        }
    }
}

// ── Interpreter: evaluate with result (eliminates boolean blindness) ────

/// Evaluation result that carries WHY a check passed or failed.
#[derive(Debug, Clone)]
pub enum RuleResult<A> {
    Pass {
        check: A,
    },
    Fail {
        check: A,
    },
    AllOf {
        results: Vec<RuleResult<A>>,
        passed: bool,
    },
    AnyOf {
        results: Vec<RuleResult<A>>,
        passed: bool,
    },
    Negated {
        inner: Box<RuleResult<A>>,
        passed: bool,
    },
}

impl<A> RuleResult<A> {
    pub fn passed(&self) -> bool {
        match self {
            Self::Pass { .. } => true,
            Self::Fail { .. } => false,
            Self::AllOf { passed, .. } => *passed,
            Self::AnyOf { passed, .. } => *passed,
            Self::Negated { passed, .. } => *passed,
        }
    }
}

impl<A: Clone> RuleResult<A> {
    /// Collect passing leaf checks from the result tree.
    fn collect_passes(&self) -> Vec<A> {
        match self {
            Self::Pass { check } => vec![check.clone()],
            Self::Fail { .. } => vec![],
            Self::AllOf { results, passed } | Self::AnyOf { results, passed } => {
                if *passed {
                    results.iter().flat_map(|r| r.collect_passes()).collect()
                } else {
                    vec![]
                }
            }
            Self::Negated { passed, .. } => {
                if *passed {
                    // Negated passed = inner failed. No passing leaves here.
                    vec![]
                } else {
                    vec![]
                }
            }
        }
    }

    /// Collect contributing failed leaf checks from the result tree.
    /// Only descends into nodes that themselves failed — a passing `AnyOf`
    /// does not surface its non-contributing child failures.
    pub fn collect_failures(&self) -> Vec<A> {
        match self {
            Self::Pass { .. } => vec![],
            Self::Fail { check } => vec![check.clone()],
            Self::AllOf { results, passed } | Self::AnyOf { results, passed } => {
                if *passed {
                    vec![]
                } else {
                    results.iter().flat_map(|r| r.collect_failures()).collect()
                }
            }
            Self::Negated { inner, passed } => {
                if *passed {
                    vec![]
                } else {
                    // Negated node failed = inner passed. Report the inner's passing
                    // checks as the "failures" — they are what the Not disagrees with.
                    inner.collect_passes()
                }
            }
        }
    }
}

impl<A: Clone + fmt::Display> RuleResult<A> {
    /// Collect failure messages as strings, ready for error reporting.
    pub fn failure_messages(&self) -> Vec<String> {
        self.collect_failures()
            .into_iter()
            .map(|a| a.to_string())
            .collect()
    }
}

impl<A: Clone> Rule<A> {
    /// Evaluate with full result tree — no boolean blindness.
    pub fn evaluate_detailed(&self, predicate: &impl Fn(&A) -> bool) -> RuleResult<A> {
        match self {
            Self::Check(a) => {
                if predicate(a) {
                    RuleResult::Pass { check: a.clone() }
                } else {
                    RuleResult::Fail { check: a.clone() }
                }
            }
            Self::All(rules) => {
                let results: Vec<_> = rules
                    .iter()
                    .map(|r| r.evaluate_detailed(predicate))
                    .collect();
                let passed = results.iter().all(|r| r.passed());
                RuleResult::AllOf { results, passed }
            }
            Self::Any(rules) => {
                let results: Vec<_> = rules
                    .iter()
                    .map(|r| r.evaluate_detailed(predicate))
                    .collect();
                let passed = results.iter().any(|r| r.passed());
                RuleResult::AnyOf { results, passed }
            }
            Self::Not(r) => {
                let inner = r.evaluate_detailed(predicate);
                let passed = !inner.passed();
                RuleResult::Negated {
                    inner: Box::new(inner),
                    passed,
                }
            }
        }
    }
}

// ── Interpreter: describe ───────────────────────────────────

impl<A: fmt::Display> Rule<A> {
    /// Generate a human-readable description of the rule tree.
    pub fn describe(&self) -> String {
        match self {
            Self::Check(a) => a.to_string(),
            Self::All(rules) => {
                let parts: Vec<_> = rules.iter().map(|r| r.describe()).collect();
                format!("({})", parts.join(" AND "))
            }
            Self::Any(rules) => {
                let parts: Vec<_> = rules.iter().map(|r| r.describe()).collect();
                format!("({})", parts.join(" OR "))
            }
            Self::Not(r) => format!("NOT {}", r.describe()),
        }
    }
}

// ── Interpreter: collect leaf checks ────────────────────────

impl<A: Clone> Rule<A> {
    /// Collect all leaf `Check` values from the tree (depth-first).
    pub fn collect_checks(&self) -> Vec<A> {
        match self {
            Self::Check(a) => vec![a.clone()],
            Self::All(rules) | Self::Any(rules) => {
                rules.iter().flat_map(|r| r.collect_checks()).collect()
            }
            Self::Not(r) => r.collect_checks(),
        }
    }
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    enum Check {
        IsAdmin,
        HasEmail,
        MinAge(u32),
    }

    impl fmt::Display for Check {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::IsAdmin => write!(f, "is admin"),
                Self::HasEmail => write!(f, "has email"),
                Self::MinAge(n) => write!(f, "age >= {n}"),
            }
        }
    }

    fn eval_check(check: &Check) -> bool {
        match check {
            Check::IsAdmin => false,
            Check::HasEmail => true,
            Check::MinAge(n) => 25 >= *n,
        }
    }

    #[test]
    fn single_check_evaluates() {
        let rule = Rule::check(Check::HasEmail);
        assert!(rule.evaluate(&eval_check));

        let rule = Rule::check(Check::IsAdmin);
        assert!(!rule.evaluate(&eval_check));
    }

    #[test]
    fn all_requires_every_child() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::check(Check::MinAge(18)),
        ]);
        assert!(rule.evaluate(&eval_check));

        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::check(Check::IsAdmin),
        ]);
        assert!(!rule.evaluate(&eval_check));
    }

    #[test]
    fn any_requires_one_child() {
        let rule = Rule::any(vec![
            Rule::check(Check::IsAdmin),
            Rule::check(Check::HasEmail),
        ]);
        assert!(rule.evaluate(&eval_check));
    }

    #[test]
    fn not_inverts() {
        let rule = Rule::not(Rule::check(Check::IsAdmin));
        assert!(rule.evaluate(&eval_check));
    }

    #[test]
    fn fluent_and_chains() {
        let rule = Rule::check(Check::HasEmail).and(Rule::check(Check::MinAge(21)));
        assert!(rule.evaluate(&eval_check));
    }

    #[test]
    fn fluent_or_chains() {
        let rule = Rule::check(Check::IsAdmin).or(Rule::check(Check::HasEmail));
        assert!(rule.evaluate(&eval_check));
    }

    #[test]
    fn describe_formats_tree() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::any(vec![
                Rule::check(Check::IsAdmin),
                Rule::check(Check::MinAge(18)),
            ]),
        ]);
        assert_eq!(rule.describe(), "(has email AND (is admin OR age >= 18))");
    }

    #[test]
    fn describe_not() {
        let rule = Rule::not(Rule::check(Check::IsAdmin));
        assert_eq!(rule.describe(), "NOT is admin");
    }

    #[test]
    fn collect_checks_flattens() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::any(vec![
                Rule::check(Check::IsAdmin),
                Rule::check(Check::MinAge(18)),
            ]),
        ]);
        let checks = rule.collect_checks();
        assert_eq!(checks.len(), 3);
    }

    #[test]
    fn evaluate_detailed_passes() {
        let rule = Rule::check(Check::HasEmail);
        let result = rule.evaluate_detailed(&eval_check);
        assert!(result.passed());
    }

    #[test]
    fn evaluate_detailed_fails_with_context() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::check(Check::IsAdmin),
        ]);
        let result = rule.evaluate_detailed(&eval_check);
        assert!(!result.passed());

        // The AllOf contains a Pass and a Fail.
        if let RuleResult::AllOf { results, .. } = result {
            assert!(results[0].passed());
            assert!(!results[1].passed());
        } else {
            panic!("Expected AllOf");
        }
    }

    #[test]
    fn empty_all_is_vacuously_true() {
        let rule: Rule<Check> = Rule::all(vec![]);
        assert!(rule.evaluate(&eval_check));
    }

    #[test]
    fn empty_any_is_vacuously_false() {
        let rule: Rule<Check> = Rule::any(vec![]);
        assert!(!rule.evaluate(&eval_check));
    }

    // ── collect_failures / failure_messages ───────────────

    #[test]
    fn collect_failures_empty_on_all_pass() {
        let rule = Rule::check(Check::HasEmail);
        let result = rule.evaluate_detailed(&eval_check);
        assert!(result.collect_failures().is_empty());
    }

    #[test]
    fn collect_failures_single_leaf() {
        let rule = Rule::check(Check::IsAdmin);
        let result = rule.evaluate_detailed(&eval_check);
        let failures = result.collect_failures();
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn collect_failures_nested() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::check(Check::IsAdmin),
            Rule::any(vec![
                Rule::check(Check::IsAdmin),
                Rule::check(Check::MinAge(99)),
            ]),
        ]);
        let result = rule.evaluate_detailed(&eval_check);
        // IsAdmin fails in All, both IsAdmin and MinAge(99) fail in Any
        let failures = result.collect_failures();
        assert_eq!(failures.len(), 3);
    }

    #[test]
    fn failure_messages_formats_display() {
        let rule = Rule::all(vec![
            Rule::check(Check::HasEmail),
            Rule::check(Check::IsAdmin),
        ]);
        let result = rule.evaluate_detailed(&eval_check);
        let msgs = result.failure_messages();
        assert_eq!(msgs, vec!["is admin"]);
    }

    // ── Property-based tests (proptest) ──────────────────

    mod prop_tests {
        use super::*;
        use proptest::prelude::*;

        // Strategy for leaf Check values.
        fn arb_check() -> impl Strategy<Value = Check> {
            prop_oneof![
                Just(Check::IsAdmin),
                Just(Check::HasEmail),
                (0u32..200).prop_map(Check::MinAge),
            ]
        }

        // Strategy for Rule<Check> trees with bounded depth.
        fn arb_rule() -> impl Strategy<Value = Rule<Check>> {
            let leaf = arb_check().prop_map(Rule::Check);
            leaf.prop_recursive(4, 64, 8, |inner| {
                prop_oneof![
                    3 => arb_check().prop_map(Rule::Check),
                    1 => inner.clone().prop_map(|r| Rule::Not(Box::new(r))),
                    1 => prop::collection::vec(inner.clone(), 0..6).prop_map(Rule::All),
                    1 => prop::collection::vec(inner, 0..6).prop_map(Rule::Any),
                ]
            })
        }

        // Three predicates: always-true, always-false, domain-specific.
        fn arb_predicate_index() -> impl Strategy<Value = u8> {
            0u8..3
        }

        fn pick_predicate(idx: u8) -> impl Fn(&Check) -> bool {
            move |check| match idx {
                0 => true,
                1 => false,
                _ => eval_check(check),
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1000))]

            // LAW 1: evaluate and evaluate_detailed agree on pass/fail.
            #[test]
            fn evaluate_agrees_with_detailed(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let bool_result = rule.evaluate(&pred);
                let detailed_result = rule.evaluate_detailed(&pred);
                prop_assert_eq!(bool_result, detailed_result.passed());
            }

            // LAW 2: All is order-independent.
            #[test]
            fn all_is_order_independent(children in prop::collection::vec(arb_rule(), 0..8), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let original = Rule::All(children.clone());
                let mut reversed = children;
                reversed.reverse();
                let flipped = Rule::All(reversed);
                prop_assert_eq!(original.evaluate(&pred), flipped.evaluate(&pred));
            }

            // LAW 3: Any is order-independent.
            #[test]
            fn any_is_order_independent(children in prop::collection::vec(arb_rule(), 0..8), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let original = Rule::Any(children.clone());
                let mut reversed = children;
                reversed.reverse();
                let flipped = Rule::Any(reversed);
                prop_assert_eq!(original.evaluate(&pred), flipped.evaluate(&pred));
            }

            // LAW 4: Double negation is identity.
            #[test]
            fn double_negation(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let double_neg = Rule::not(Rule::not(rule.clone()));
                prop_assert_eq!(rule.evaluate(&pred), double_neg.evaluate(&pred));
            }

            // LAW 5: Not inverts the result.
            #[test]
            fn not_inverts_result(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let negated = Rule::not(rule.clone());
                prop_assert_ne!(rule.evaluate(&pred), negated.evaluate(&pred));
            }

            // LAW 6a: passed → no failures.
            #[test]
            fn passed_implies_no_failures(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let result = rule.evaluate_detailed(&pred);
                if result.passed() {
                    prop_assert!(
                        result.collect_failures().is_empty(),
                        "passed but failures={:?}",
                        result.collect_failures()
                    );
                }
            }

            // LAW 6b: has failures → not passed.
            #[test]
            fn failures_implies_not_passed(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let result = rule.evaluate_detailed(&pred);
                if !result.collect_failures().is_empty() {
                    prop_assert!(
                        !result.passed(),
                        "has failures but passed"
                    );
                }
            }

            // LAW 7: describe never panics and is non-empty.
            #[test]
            fn describe_never_panics(rule in arb_rule()) {
                let desc = rule.describe();
                prop_assert!(!desc.is_empty());
            }

            // LAW 8: collect_checks count is stable.
            #[test]
            fn collect_checks_count_stable(rule in arb_rule()) {
                let first = rule.collect_checks().len();
                let second = rule.collect_checks().len();
                prop_assert_eq!(first, second);
            }

            // LAW 9: failure_messages matches collect_failures count.
            #[test]
            fn failure_messages_matches_failures(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let result = rule.evaluate_detailed(&pred);
                prop_assert_eq!(
                    result.collect_failures().len(),
                    result.failure_messages().len()
                );
            }

            // LAW 10: All(single child) == child.
            #[test]
            fn all_single_child_is_identity(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let wrapped = Rule::All(vec![rule.clone()]);
                prop_assert_eq!(rule.evaluate(&pred), wrapped.evaluate(&pred));
            }

            // LAW 11: Any(single child) == child.
            #[test]
            fn any_single_child_is_identity(rule in arb_rule(), pred_idx in arb_predicate_index()) {
                let pred = pick_predicate(pred_idx);
                let wrapped = Rule::Any(vec![rule.clone()]);
                prop_assert_eq!(rule.evaluate(&pred), wrapped.evaluate(&pred));
            }
        }
    }
}
