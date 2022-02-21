SHELL:=/bin/bash

.DEFAULT_GOAL := default
.PHONY: fix fmt lint check build release test pre-commit install default clean

fix:
	cargo fix --allow-staged

fmt:
	cargo fmt

lint:
	cargo clippy
	-cargo audit

# "This will essentially compile the packages without performing the final step of code generation, which is faster than running cargo build."
check:
	cargo check

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

pre-commit: fix fmt lint test release

install:
	cargo install --force --path .

default: build

clean:
	cargo clean