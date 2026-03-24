# ADR-002: UUID v7 as primary keys

**Date:** 2026-02-23
**Status:** Accepted

## Context

Need a primary key strategy across all services. Options: auto-increment integers, UUID v4, UUID v7, ULID.

## Decision

Use UUID v7 for all primary keys.

## Consequences

- Time-ordered: natural sort order, good B-tree index locality (unlike v4 which fragments indexes)
- No coordination needed across services — IDs can be generated client-side if necessary
- 128-bit — larger than integers but standard across all Postgres/SQLx tooling
- Keyset pagination works naturally since UUID v7 is monotonically increasing
- Cursor-based pagination in the `shared` module relies on this ordering
