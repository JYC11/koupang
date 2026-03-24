# ADR-007: Money Handling with rust_decimal and NUMERIC(19,4)

**Date:** 2026-02-24
**Status:** Accepted

## Context

The Catalog service handles product prices and SKU pricing. Floating-point types (f32/f64) cause rounding errors that are unacceptable for financial data. We need a precise decimal representation both in Rust and PostgreSQL.

## Decision

- Use `rust_decimal::Decimal` in Rust for all monetary values (prices, amounts).
- Use `NUMERIC(19,4)` in PostgreSQL — 19 total digits with 4 decimal places, supporting values up to ~999 trillion with sub-cent precision.
- Store currency as `VARCHAR(3)` (ISO 4217 codes), defaulting to `USD`.
- Validate via `Price` value object that rejects negative values.
- Enable `rust_decimal` feature in sqlx for seamless DB round-tripping.

## Consequences

- **Easier:** No floating-point rounding bugs. Exact decimal arithmetic across the stack. sqlx handles Decimal ↔ NUMERIC conversion automatically.
- **Harder:** `rust_decimal` adds a dependency. Arithmetic operations require using Decimal methods rather than native operators (though basic ops are implemented via traits).
- **Trade-off:** NUMERIC(19,4) limits precision to 4 decimal places — sufficient for most currencies but would need revisiting for crypto or high-precision financial instruments.
