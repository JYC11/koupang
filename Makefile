.PHONY: local-infra
local-infra: ## Setup local infrastructure with docker compose
	 docker compose -f docker-compose.infra.yml up -d

.PHONY: local-infra-down
local-infra-down: ## Setup local infrastructure with docker compose
	 docker compose -f docker-compose.infra.yml down

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