# Project

- an ecommerce microservice project for learning and portfolio purposes

## Features

- refer to .plan/critical-user-flows.md for user flows as needed

## Documentation reminder
- when changes are made, update the corresponding CLAUDE.md file in the relevant module
- when a non-trivial architectural or technical decision is made during a plan, create an ADR in `.plan/adr/` using `make adr` or the template at `.plan/adr/template.md`

## Progress tracking
- ADRs (Architecture Decision Records) live in `.plan/adr/` — one file per decision capturing context, decision, and consequences
- Git tags mark milestones (e.g. `v0.1-identity-auth`) — tag after completing a service or major feature
- Progress summaries for blog posts live in `.plan/progress-summary-*.md`

## Microservices

- Identity
  - Responsibility: Auth, Users, Profiles
  - Data owned: Users, Credentials, Roles
  - See [identity/CLAUDE.md](identity/CLAUDE.md) for full reference
- Catalog
  - Responsibility: Product Info, Pricing, Inventory
  - Data owned: Products, stock levels
- Order
  - Responsibility: Order lifcycle (state machine)
  - Data owned: Orders, order items
- Payment
  - Responsibility: Payament gateway integration, wallets
  - Data owned: Transactions, Invoices
- Shipping
  - Responsibility: Logistics, tracking, addresses
  - Data owned: Shipments, Carriers
- Notification
  - Responsibility: Emails, SMS, Push
  - Data owned: Templates, Delivery Logs
- Review
  - Responsibility: Product reviews
  - Data owned: Review
- Moderation
  - Responsiblity: Moderating seller products and buyer reviews
  - Data owned: Moderation log
- Shared
  - Responsibility: contains shared libraries/code between services
  - See [shared/CLAUDE.md](shared/CLAUDE.md) for full module reference

## Tech stack

- rust (most used crates in no particular order)
  - axum
  - sqlx
  - tokio
- infra
  - postgres
    - using version 18
    - using uuid v7 as a primary key
  - redis
- containerization
  - docker
  - docker compose
- monitoring/observability
  - opentelemetry
  - prometheus
- message queue
- kafka

## Patterns to implement

- Api Versioning
- Event Driven Architecture: https://crates.io/crates/ruva
- Transactional Outbox: https://crates.io/crates/outbox-core
- Listen to yourself
- Resilience: https://crates.io/crates/failsafe
- Observability
- Idempotency
- API gateway/BFF
- Background jobs: https://crates.io/crates/aj
- CQRS

## Scripts
- refer to the Makefile
  - `make test SERVICE=identity` — run tests for a service
  - `make migration SERVICE=identity` — create a new migration file
  - `make adr` — create a new ADR file (auto-increments number)
  - `make local-infra` / `make local-infra-down` — start/stop local Docker infra

## Prompt logging

- At the END of every session, log all user prompts from this session to the memory folder under `llm_usage_logging_folder/`
- Format: append to a file named by date (e.g. `session-log-2026-02-25.md`)
- Each entry should include: session start time, numbered user prompts (just the raw text, no system messages), and a 1-line summary of what was accomplished
- This is for blogging purposes — the logs will be used in blog posts about LLM usage

## Task management

- beads_rust: https://github.com/Dicklesworthstone/beads_rust
  - br skill has been created for use
  - load in this skill first and then create tasks when plan is approved
