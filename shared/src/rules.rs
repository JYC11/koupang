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
    /// Collect all failed leaf checks from the result tree.
    pub fn collect_failures(&self) -> Vec<A> {
        match self {
            Self::Pass { .. } => vec![],
            Self::Fail { check } => vec![check.clone()],
            Self::AllOf { results, .. } | Self::AnyOf { results, .. } => {
                results.iter().flat_map(|r| r.collect_failures()).collect()
            }
            Self::Negated { inner, passed } => {
                if *passed {
                    vec![]
                } else {
                    inner.collect_failures()
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
}
