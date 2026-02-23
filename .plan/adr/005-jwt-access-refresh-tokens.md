# ADR-005: JWT with access + refresh token pair

**Date:** 2026-02-23
**Status:** Accepted

## Context

Need an authentication mechanism for the API. Options: session cookies, opaque tokens with DB lookup, JWT only, JWT with refresh tokens.

## Decision

Use short-lived JWT access tokens + longer-lived JWT refresh tokens. Access tokens carry claims (user ID, username, role). Refresh tokens carry only user ID and a JTI. Both secrets are configured via environment variables.

## Consequences

- Access tokens are stateless — no DB lookup on every request, good for microservice-to-microservice calls
- Refresh tokens allow re-issuing access tokens without re-authenticating
- Token revocation is not instant (access token lives until expiry) — acceptable tradeoff for now
- Future: can add a Redis deny-list for revoked access tokens if needed
- gRPC service uses the same JWT validation for inter-service auth
