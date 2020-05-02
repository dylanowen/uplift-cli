SHELL:=/bin/bash

.DEFAULT_GOAL := default

format:
	cargo fmt

clippy:
	cargo clippy

build: format clippy
	cargo build

install: build
	cargo install --force --path .

default: build

clean:
	cargo clean