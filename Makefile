.PHONY: local-infra
local-infra: ## Setup local infrastructure with docker compose
	 docker compose -f docker-compose.infra.yml up -d

.PHONY: local-infra-down
local-infra-down: ## Setup local infrastructure with docker compose
	 docker compose -f docker-compose.infra.yml down

.PHONY: fmt
fmt: ## Format a service (usage: make fmt SERVICE=identity, make fmt SERVICE=all CHECK=1)
	@if [ -z "$(SERVICE)" ]; then \
		echo "Usage: make fmt SERVICE=<service-name|all> [CHECK=1]"; \
		echo ""; \
		echo "Available services: shared, identity, catalog, order, payment, shipping, notification, review, moderation, all"; \
		exit 1; \
	fi
	@bash util-scripts/fmt.sh $(SERVICE) $(if $(CHECK),--check)

.PHONY: check
check: ## Type-check a service with optional clippy (usage: make check SERVICE=identity, make check SERVICE=all CLIPPY=1)
	@if [ -z "$(SERVICE)" ]; then \
		echo "Usage: make check SERVICE=<service-name|all> [CLIPPY=1]"; \
		echo ""; \
		echo "Available services: shared, identity, catalog, order, payment, shipping, notification, review, moderation, all"; \
		exit 1; \
	fi
	@bash util-scripts/check.sh $(SERVICE) $(if $(CLIPPY),--clippy)

.PHONY: build
build: ## Build a service (usage: make build SERVICE=identity, make build SERVICE=all RELEASE=1)
	@if [ -z "$(SERVICE)" ]; then \
		echo "Usage: make build SERVICE=<service-name|all> [RELEASE=1]"; \
		echo ""; \
		echo "Available services: shared, identity, catalog, order, payment, shipping, notification, review, moderation, all"; \
		exit 1; \
	fi
	@bash util-scripts/build.sh $(SERVICE) $(if $(RELEASE),--release)

.PHONY: run
run: ## Run a specific service locally (usage: make run SERVICE=identity)
	@if [ -z "$(SERVICE)" ]; then \
		echo "Usage: make run SERVICE=<service-name>"; \
		echo ""; \
		echo "Available services: identity, catalog, order, payment, shipping, notification, review, moderation"; \
		exit 1; \
	fi
	@bash util-scripts/run.sh $(SERVICE)

.PHONY: test
test: ## Run tests for a specific service or all services (usage: make test SERVICE=identity or make test SERVICE=all)
	@if [ -z "$(SERVICE)" ]; then \
		echo "Usage: make test SERVICE=<service-name|all>"; \
		echo ""; \
		echo "Available services: identity, catalog, order, payment, shipping, notification, review, moderation, all"; \
		exit 1; \
	fi
	@bash util-scripts/test.sh $(SERVICE)

.PHONY: migration
migration: ## Create a new migration file (usage: make migration SERVICE=identity NAME=init)
	@if [ -z "$(SERVICE)" ] || [ -z "$(NAME)" ]; then \
		bash util-scripts/migration.sh; \
	else \
		bash util-scripts/migration.sh $(SERVICE) $(NAME); \
	fi

.PHONY: adr
adr: ## Create a new ADR file (usage: make adr or make adr TITLE="use redis for caching")
	@bash util-scripts/adr.sh $(TITLE)