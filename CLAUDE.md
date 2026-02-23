# Project

- an ecommerce microservice project for learning and portfolio purposes

## Features

- refer to .plan/critical-user-flows.md for user flows as needed

## Microservices

- Identity
  - Responsibility: Auth, Users, Profiles
  - Data owned: Users, Credentials, Roles
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

## Tech stack

- rust (most used crates in no particular order)
  - axum
  - sqlx
  - tokio
- infra
  - postgres
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

## Task management

- beads_rust: https://github.com/Dicklesworthstone/beads_rust
  - br skill has been created for use
