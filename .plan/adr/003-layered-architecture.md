# ADR-003: Layered architecture (routes → service → repository)

**Date:** 2026-02-23
**Status:** Accepted

## Context

Need a consistent code structure within each microservice. Options: flat handlers with inline SQL, hexagonal/ports-and-adapters, layered architecture.

## Decision

Each service follows: routes (HTTP handlers) → service (business logic) → repository (SQL queries). DTOs live alongside the feature module. The shared crate provides infrastructure (auth, DB, errors) but not business logic.
Chose this over more complex hexagonal architecture because it doesn't seem to add much value at this point.

## Consequences

- Repository functions take a generic executor (`PgExec` or `TxContext`) so they work with both direct queries and transactions
- Service layer owns basic logic (password hashing, token generation, email sending, caching) — handlers stay thin
   - More complex domain logic can have a separate layer, e.g. `domain` crate, but this is not required yet
- Testing at each layer is straightforward: repository tests hit DB directly, service tests use real DB + mock email, router tests go through HTTP
- Adding a new feature module means creating the same file structure (entities, dtos, repository, service, routes)
