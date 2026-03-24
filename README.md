# Koupang

An ecommerce microservice platform built in Rust, designed for learning and portfolio purposes with production-quality patterns.

## Tech Stack

- **Language:** Rust (axum, sqlx, tokio)
- **Databases:** Postgres 18 (UUID v7 PKs), Redis
- **Messaging:** Kafka (transactional outbox pattern)
- **Testing:** Testcontainers (real Postgres/Redis/Kafka in tests)
- **Containers:** Docker Compose

## Services

| Service | Status | Responsibility |
|---------|--------|---------------|
| **shared** | Complete | Shared libraries (auth, DB, events, outbox, rules, distributed lock) |
| **identity** | Complete (84 tests) | Auth, users, profiles |
| **catalog** | Complete (160 tests) | Products, pricing, inventory, categories, brands |
| **order** | Complete (88 tests) | Order lifecycle (state machine) |
| **payment** | Complete (88 tests) | Payment gateway, double-entry ledger, capture retry |
| **cart** | Complete (59 tests) | Shopping cart (Redis) |
| **saga-tests** | Complete (6 tests) | Cross-service saga integration tests |
| shipping | Stub | Logistics, tracking |
| notification | Stub | Emails, SMS, push |
| review | Stub | Product reviews |
| moderation | Stub | Content moderation |
| bff-gateway | Stub | API gateway |

## Architecture

Choreography-based saga for the ordering flow (no central orchestrator). Each service consumes Kafka events and publishes its own via the transactional outbox pattern. See [docs/ordering-saga-flows.md](docs/ordering-saga-flows.md) for the full flow documentation.

Key patterns: layered architecture (routes -> service -> domain -> repository), data-oriented programming with composable business rules, claims-based JWT auth, double-entry ledger for payments, distributed locking for concurrency.

## Documentation

| Document | Description |
|----------|-------------|
| [CLAUDE.md](CLAUDE.md) | Project overview, imports, scripts, ADR index |
| [STYLE.md](STYLE.md) | Coding conventions, DOP principles, testing rules |
| [docs/ordering-saga-flows.md](docs/ordering-saga-flows.md) | Saga event flows, state machine, payload schemas |
| [docs/adr/](docs/adr/) | Architecture Decision Records (12 decisions) |

## Development

```bash
make fmt SERVICE=identity        # format a service
make check SERVICE=identity      # type-check (CLIPPY=1 for lints)
make test SERVICE=identity       # run tests
make test SERVICE=all            # run all tests
make local-infra                 # start Docker infra
make adr                         # create a new ADR
```

## Credits

Built with AI assistance (Claude). Critiques, suggestions, and reviews welcome.
