SHELL := /bin/bash

NS ?= default
DATA_FILE ?=
HOST ?= 127.0.0.1
PORT ?= 7878
HTML_OUT ?= kvstore-view.html
LIMIT ?= 10
QUERY ?=
KEY ?=
VALUE ?=
FILE ?=
TAGS ?=
ARGS ?=
CARGO_TOOLS_BIN ?= ./target/cargo-tools/bin
AUDIT_DB ?= ./target/advisory-db
AUDIT_FLAGS ?= --db $(AUDIT_DB) --no-fetch --stale

COMMON_FLAGS := $(if $(strip $(NS)),-n $(NS),) $(if $(strip $(DATA_FILE)),--data-file $(DATA_FILE),)

.PHONY: help build fmt check test clippy audit install run serve shutdown html list get add remove search recent export import put-file get-file

help:
	@echo "kvstore shortcuts"
	@echo ""
	@echo "General:"
	@echo "  make build"
	@echo "  make fmt"
	@echo "  make check"
	@echo "  make test"
	@echo "  make clippy"
	@echo "  make audit"
	@echo "  make install"
	@echo ""
	@echo "Run arbitrary command:"
	@echo "  make run NS=work ARGS='list'"
	@echo ""
	@echo "High-level commands:"
	@echo "  make serve NS=work PORT=7878"
	@echo "  make shutdown PORT=7878"
	@echo "  make html NS=work HTML_OUT=work-view.html"
	@echo "  make add NS=work KEY=todo VALUE='ship v1' TAGS='@roadmap @priority'"
	@echo "  make get NS=work KEY=todo"
	@echo "  make remove NS=work KEY=todo"
	@echo "  make search NS=work QUERY=todo LIMIT=5"
	@echo "  make put-file NS=work KEY=project_summary FILE=./notes/summary.md TAGS='@codex @summary'"
	@echo "  make get-file NS=work KEY=project_summary FILE=./notes/summary_out.md"
	@echo ""
	@echo "Variables: NS DATA_FILE HOST PORT HTML_OUT LIMIT QUERY KEY VALUE FILE TAGS ARGS"

build:
	cargo build

fmt:
	cargo fmt

check:
	cargo check

test:
	cargo test

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

audit:
	@if command -v cargo-audit >/dev/null 2>&1; then \
		cargo-audit audit $(AUDIT_FLAGS); \
	elif [ -x "$(CARGO_TOOLS_BIN)/cargo-audit" ]; then \
		"$(CARGO_TOOLS_BIN)/cargo-audit" audit $(AUDIT_FLAGS); \
	else \
		echo "cargo-audit not found. Install with: cargo install cargo-audit"; \
		echo "or install locally with: cargo install cargo-audit --root ./target/cargo-tools"; \
		exit 1; \
	fi

install:
	cargo install --path . --force

run:
	cargo run -- $(COMMON_FLAGS) $(ARGS)

serve:
	cargo run -- $(COMMON_FLAGS) serve --host $(HOST) --port $(PORT)

shutdown:
	@pids=$$(lsof -tiTCP:$(PORT) -sTCP:LISTEN 2>/dev/null); \
	if [ -z "$$pids" ]; then \
		echo "No listening process found on port $(PORT)."; \
	else \
		echo "Stopping process(es) on port $(PORT): $$pids"; \
		kill $$pids; \
	fi

html:
	cargo run -- $(COMMON_FLAGS) html --path $(HTML_OUT)

list:
	cargo run -- $(COMMON_FLAGS) list

get:
	@test -n "$(KEY)" || (echo "KEY is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) get $(KEY)

add:
	@test -n "$(KEY)" || (echo "KEY is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) add $(KEY) "$(VALUE)" $(TAGS)

remove:
	@test -n "$(KEY)" || (echo "KEY is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) remove $(KEY)

search:
	@test -n "$(QUERY)" || (echo "QUERY is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) search $(QUERY) --limit $(LIMIT)

recent:
	cargo run -- $(COMMON_FLAGS) recent --limit $(LIMIT)

export:
	@test -n "$(FILE)" || (echo "FILE is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) export $(FILE)

import:
	@test -n "$(FILE)" || (echo "FILE is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) import $(FILE)

put-file:
	@test -n "$(KEY)" || (echo "KEY is required" && exit 1)
	@test -n "$(FILE)" || (echo "FILE is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) put-file $(KEY) $(FILE) $(TAGS)

get-file:
	@test -n "$(KEY)" || (echo "KEY is required" && exit 1)
	@test -n "$(FILE)" || (echo "FILE is required" && exit 1)
	cargo run -- $(COMMON_FLAGS) get-file $(KEY) $(FILE)
