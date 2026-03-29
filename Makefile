# Airwaves OS - Development Makefile

ARMBIAN_BUILD_TAG ?= v25.02
CONTROL_APP_REPO ?= https://github.com/airframesio/airwaves-os-control

.PHONY: help build-image build-containers dev dev-up dev-down dev-logs clean

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

build-rpi5: ## Build for Raspberry Pi 5
	$(MAKE) build-image BOARD=rpi5b RELEASE=noble

build-rock5b: ## Build for Rock 5B
	$(MAKE) build-image BOARD=rock-5b RELEASE=bookworm

build-opi5: ## Build for Orange Pi 5
	$(MAKE) build-image BOARD=orangepi5 RELEASE=bookworm

# ──── Container Builds ────

build-gateway: ## Build the gateway container
	cd containers/airwaves-gateway && ./build.sh

build-manager: ## Build the manager container
	docker build -t airwaves-manager:latest containers/airwaves-manager

build-containers: build-manager ## Build all containers
	@echo "All containers built"

# ──── Development ────

dev-up: ## Start the dev stack (manager container)
	@mkdir -p dev/config
	@test -f dev/config/config.json || cp armbian/userpatches/extensions/airwaves-os/config/templates/config.json.template dev/config/config.json
	@test -f dev/config/catalog.json || cp armbian/userpatches/extensions/airwaves-os/config/catalog.json dev/config/catalog.json
	docker compose -f docker-compose.dev.yml up -d --build

dev-down: ## Stop the dev stack
	docker compose -f docker-compose.dev.yml down

dev-logs: ## Follow dev stack logs
	docker compose -f docker-compose.dev.yml logs -f

dev: dev-up ## Start dev stack and show logs
	@echo ""
	@echo "Manager API running at http://localhost:8080"
	@echo "Run your control app with: cd ../airwaves-os-control && npm run dev"
	@echo "The Vite proxy will forward /api/v1/* to the manager."
	@echo ""
	docker compose -f docker-compose.dev.yml logs -f

# ──── Rust Manager Development ────

check: ## Run cargo check on the manager
	cd containers/airwaves-manager && cargo check

test: ## Run cargo test on the manager
	cd containers/airwaves-manager && cargo test

clippy: ## Run clippy lints on the manager
	cd containers/airwaves-manager && cargo clippy -- -W clippy::all

# ──── Cleanup ────

clean: ## Remove build artifacts
	rm -rf .armbian-build
	docker compose -f docker-compose.dev.yml down -v 2>/dev/null || true
	@echo "Cleaned"
