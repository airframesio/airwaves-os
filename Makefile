# Airwaves OS - Development Makefile

# The Armbian build framework tag is pinned in armbian/build.sh (single source
# of truth, currently v26.2.1). Override for a one-off build with:
#   make build-image ARMBIAN_BUILD_TAG=vX.Y.Z
# (command-line variables are exported to the build script's environment).
CONTROL_APP_REPO ?= https://github.com/airframesio/airwaves-os-control
CONTROL_APP_LOCAL ?= ../airwaves-os-control

.PHONY: help build-image build-containers dev dev-up dev-down dev-logs prod prod-up prod-down clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ──── OS Image Builds ────

build-image: ## Build OS image (BOARD=rock-5b RELEASE=bookworm)
	@echo "Building Airwaves OS image..."
	./armbian/build.sh airwaves BOARD=$(BOARD) BRANCH=current RELEASE=$(RELEASE)

build-x86: ## Build for x86 UEFI (mini PC / server)
	$(MAKE) build-image BOARD=uefi-x86 RELEASE=bookworm

build-rpi4: ## Build for Raspberry Pi 4B
	$(MAKE) build-image BOARD=rpi4b RELEASE=noble

build-rpi5: ## Build for Raspberry Pi 5 (same rpi4b image; Armbian's rpi4b target covers Pi 3-5)
	$(MAKE) build-image BOARD=rpi4b RELEASE=noble

build-rock5b: ## Build for Rock 5B
	$(MAKE) build-image BOARD=rock-5b RELEASE=bookworm

build-opi5: ## Build for Orange Pi 5
	$(MAKE) build-image BOARD=orangepi5 RELEASE=bookworm

# ──── Container Builds ────

build-gateway: ## Build gateway with control app bundled
	@if [ -d "$(CONTROL_APP_LOCAL)" ]; then \
		echo "==> Copying control app from $(CONTROL_APP_LOCAL)"; \
		rm -rf containers/airwaves-gateway/control-app; \
		cp -r "$(CONTROL_APP_LOCAL)" containers/airwaves-gateway/control-app; \
	else \
		echo "==> Control app not found locally, gateway will use landing page"; \
	fi
	docker build -t airwaves-gateway:latest containers/airwaves-gateway
	@rm -rf containers/airwaves-gateway/control-app

build-manager: ## Build the manager container
	docker build -t airwaves-manager:dev containers/airwaves-manager

build-containers: build-manager build-gateway ## Build all containers

# ──── Development (manager only, control app via Vite) ────

dev-up: ## Start the dev stack (manager only at :8080)
	@mkdir -p dev/config
	@test -f dev/config/config.json || cp armbian/userpatches/extensions/airwaves-os/config/templates/config.json.template dev/config/config.json
	@test -f dev/config/catalog.json || cp armbian/userpatches/extensions/airwaves-os/config/catalog.json dev/config/catalog.json
	docker compose -f docker-compose.dev.yml up -d --build

dev-down: ## Stop the dev stack
	docker compose -f docker-compose.dev.yml down

dev-logs: ## Follow dev stack logs
	docker compose -f docker-compose.dev.yml logs -f

dev: dev-up ## Start dev stack and show instructions
	@echo ""
	@echo "Manager API: http://localhost:8080"
	@echo "Control app: cd $(CONTROL_APP_LOCAL) && npm run dev"
	@echo "Vite proxy forwards /api/v1/* to manager automatically."
	@echo ""

# ──── Production (full stack: gateway + manager on :80) ────

prod-up: build-containers ## Build and start full production stack at :80
	@mkdir -p dev/config
	@test -f dev/config/config.json || cp armbian/userpatches/extensions/airwaves-os/config/templates/config.json.template dev/config/config.json
	@test -f dev/config/catalog.json || cp armbian/userpatches/extensions/airwaves-os/config/catalog.json dev/config/catalog.json
	docker compose -f docker-compose.prod.yml up -d
	@echo ""
	@echo "Airwaves OS running at http://localhost"
	@echo ""

prod-down: ## Stop the production stack
	docker compose -f docker-compose.prod.yml down

prod-logs: ## Follow production stack logs
	docker compose -f docker-compose.prod.yml logs -f

# ──── Rust Manager Development ────

check: ## Run cargo check on the manager
	cd containers/airwaves-manager && cargo check

test: ## Run cargo test on the manager
	cd containers/airwaves-manager && cargo test

clippy: ## Run clippy lints on the manager
	cd containers/airwaves-manager && cargo clippy -- -W clippy::all

# ──── Cleanup ────

clean: ## Remove build artifacts and stop containers
	rm -rf .armbian-build
	docker compose -f docker-compose.dev.yml down -v 2>/dev/null || true
	docker compose -f docker-compose.prod.yml down -v 2>/dev/null || true
	rm -rf containers/airwaves-gateway/control-app
	@echo "Cleaned"
