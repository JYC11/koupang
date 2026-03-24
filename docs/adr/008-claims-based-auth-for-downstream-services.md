# ADR-008: Claims-Based Auth for Downstream Services

**Date:** 2026-02-24
**Status:** Accepted

## Context

The Identity service owns the users table and validates JWTs by looking up the user in the database (`AuthMiddleware::new()` with `GetCurrentUser` trait). Downstream services (Catalog, Order, etc.) don't have access to the users table and shouldn't need a database call or gRPC call on every authenticated request.

## Decision

- Add `AuthMiddleware::new_claims_based(jwt_service)` constructor to shared middleware.
- When `trust_claims` mode is active, `CurrentUser` is constructed directly from the JWT's `AccessTokenClaims` (sub → id, role → role) without calling `get_by_id()`.
- Identity service continues using `AuthMiddleware::new()` with the full DB lookup.
- Backward-compatible: no changes to Identity's existing code.

## Consequences

- **Easier:** Downstream services authenticate requests with zero additional network calls or DB queries. Adding auth to a new service is a one-liner (`AuthMiddleware::new_claims_based(jwt_service)`).
- **Harder:** If a user's role changes or they're banned, downstream services won't know until the JWT expires. This is acceptable for short-lived access tokens (default 15-30 min).
- **Future:** ADR for resilient auth (gRPC to Identity + Redis cache + circuit breaker) is tracked as `bd-dsh` for services that need stronger consistency guarantees.
