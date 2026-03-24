# ADR-006: Email as a trait with mock implementation

**Date:** 2026-02-23
**Status:** Accepted

## Context

Identity service needs to send verification and password reset emails. Don't want to couple to a specific email provider during early development.

## Decision

Define an `EmailService` trait in the shared crate. Provide a `MockEmailService` that logs emails via tracing. Services depend on the trait, not the implementation.

## Consequences

- Can develop and test the full registration + password reset flow without an email provider
- Swapping in SendGrid/Mailgun/SES later means implementing one trait method
- Mock is also used in tests — no need for a separate test double
- Downside: can't verify actual email delivery in integration tests yet
