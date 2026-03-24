# ADR-009: Postgres ltree for Category Hierarchy

**Date:** 2026-02-25
**Status:** Accepted

## Context

The catalog service needs a category hierarchy (e.g. Electronics > Smart Phones > Android). Common approaches for storing trees in a relational database include adjacency lists, nested sets, closure tables, and materialized paths. We need efficient queries for:

- Fetching direct children of a category
- Fetching the entire subtree under a category
- Fetching all ancestors of a category (breadcrumb navigation)
- Ensuring referential integrity (no orphans, no deleting categories with children or products)

## Decision

- Use the **Postgres ltree extension** for materialized path storage.
- Each category stores a `path` column of type `ltree` (e.g. `electronics.smart_phones.android`), a `parent_id` FK for direct parent reference, and a `depth` integer for convenience.
- `LtreeLabel` value object converts human names to ltree-safe labels: `"Smart Phones"` → `"smart_phones"` (lowercase alphanumeric + underscores, must start with letter).
- Path is computed on creation: root categories use `{label}`, children use `{parent.path}.{label}`.
- Tree queries use ltree operators:
  - Subtree: `WHERE path <@ $1::ltree` (all descendants)
  - Ancestors: `WHERE path @> $1::ltree` (all ancestors up to root)
- Delete guards in the service layer: refuse to delete categories that have children (`has_children()`) or products (`has_products()`).
- Categories and brands are admin-only for mutations; reads are public.

## Consequences

- **Easier:** Subtree and ancestor queries are single indexed operations (`<@`, `@>`), no recursive CTEs needed. Breadcrumb generation is trivial.
- **Harder:** Moving a category to a different parent requires updating the `path` of all descendants (batch `UPDATE`). Not yet implemented but straightforward with `SET path = new_prefix || subpath(path, nlevel(old_prefix))`.
- **Trade-off:** ltree is a Postgres-specific extension — not portable to other databases. Acceptable since Postgres is the chosen database (ADR-002).
- **Future:** Category moves, path-based filtering in product search, and category-scoped aggregations.
