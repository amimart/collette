# Constants:
TARGET_FOLDER       = target

BOLD   := \033[1m
COLOR_RESET  = $(call get_color,sgr0,)
COLOR_CYAN   = $(call get_color,setaf,6)
COLOR_GREEN  = $(call get_color,setaf,2)
COLOR_YELLOW = $(call get_color,setaf,3)

# Some colors (if supported)
define get_color
$(shell tput -Txterm $(1) $(2) 2>/dev/null || echo "")
endef

.PHONY: all
all: help

## Build:
.PHONY: build
build: build-rust ## Build rust (i.e. not docker)

.PHONY: build-rust
build-rust: ## Build the Rust sources
	@printf "$(COLOR_GREEN)$(BOLD)🔨 Building...$(COLOR_RESET)\n"
	cargo build --all-features

## Test:
.PHONY: test
test: test-rust ## Run all tests

.PHONY: test-rust
test-rust: ## Run Rust tests
	@printf "$(COLOR_GREEN)$(BOLD)🧪 Running tests...$(COLOR_RESET)\n"
	cargo test --doc --all-features

## Lint:
.PHONY: lint
lint: lint-rust lint-rust-format lint-md lint-yaml ## Run all linters

.PHONY: lint-rust
lint-rust: ## Lint Rust code with Clippy
	@printf "$(COLOR_CYAN)$(BOLD)🦀 Linting Rust...$(COLOR_RESET)\n"
	cargo clippy -- -D warnings

.PHONY: lint-rust-format
lint-rust-format: ## Lint Rust formatting
	@printf "$(COLOR_CYAN)$(BOLD)🎨 Checking formatting...$(COLOR_RESET)\n"
	cargo fmt --check

.PHONY: lint-md
lint-md: ## Lint Markdown
	@printf "$(COLOR_CYAN)$(BOLD)📝 Linting Markdown...$(COLOR_RESET)\n"
	npx markdownlint-cli2 "**/*.md"

.PHONY: lint-yaml
lint-yaml: ## Lint YAML
	@printf "$(COLOR_CYAN)$(BOLD)📋 Linting YAML...$(COLOR_RESET)\n"
	yamllint .

## Checks:
.PHONY: check
check: lint build test audit ## Run all checks

.PHONY: audit
audit: ## Run security audit
	@printf "$(COLOR_YELLOW)$(BOLD)🔒 Running security audit...$(COLOR_RESET)\n"
	cargo audit

## Fix:
.PHONY: fix
fix: fix-rust fix-md ## Auto-fix all (yaml has no auto-fixer)

.PHONY: fix-rust
fix-rust: ## Auto-fix Rust formatting and clippy lints
	@printf "$(COLOR_CYAN)$(BOLD)🔧 Fixing Rust...$(COLOR_RESET)\n"
	cargo fmt
	cargo clippy --fix --allow-dirty

.PHONY: fix-md
fix-md: ## Auto-fix Markdown
	@printf "$(COLOR_CYAN)$(BOLD)🔧 Fixing Markdown...$(COLOR_RESET)\n"
	npx markdownlint-cli2 --fix "**/*.md"

## Clean:
.PHONY: clean
clean: ## Clean the build artifacts
	rm -rf $(TARGET_FOLDER)

## Help:
.PHONY: help
help: ## Show this help
	@echo ''
	@echo 'Usage:'
	@echo '  ${COLOR_YELLOW}make${COLOR_RESET} ${COLOR_GREEN}<target>${COLOR_RESET}'
	@echo ''
	@echo 'Targets:'
	@$(foreach V,$(sort $(.VARIABLES)), \
		$(if $(filter-out environment% default automatic,$(origin $V)), \
			$(if $(filter TOOL_%,$V), \
				export $V="$($V)";))) \
	awk 'BEGIN {FS = ":.*?## "} { \
		if (/^[a-zA-Z_-]+:.*?##.*$$/) {printf "    ${COLOR_YELLOW}%-20s${COLOR_GREEN}%s${COLOR_RESET}\n", $$1, $$2} \
		else if (/^## .*$$/) {printf "  ${COLOR_CYAN}%s${COLOR_RESET}\n", substr($$1,4)} \
		}' $(MAKEFILE_LIST) | envsubst
