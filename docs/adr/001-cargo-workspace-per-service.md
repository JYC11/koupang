# ADR-001: Cargo workspace with one crate per microservice

**Date:** 2026-02-23
**Status:** Accepted

## Context

Need a project structure for an ecommerce backend with multiple microservices. Options: monorepo with cargo workspace, separate repos per service, or a single binary with feature flags.

## Decision

Use a single Cargo workspace with each microservice as its own crate, plus a `shared` library crate for common code. Each service has its own `migrations/` directory and database.

## Consequences

- Shared code changes are immediately available to all services without publishing
- Single `cargo build` compiles everything — slower full builds but fast incremental
- Refactoring across services is easier (one PR, one commit)
- All services must agree on dependency versions (workspace resolver handles this)
- Database-per-service isolation is enforced at the Docker init script level, not by the build system
