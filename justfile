
target_config := '--all-targets --all-features'

default: build

fix:
	cargo fix {{target_config}} --allow-staged
	cargo clippy --fix {{target_config}} --allow-staged

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy {{target_config}} -- -D warnings
	-cargo audit

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

clean:
	cargo clean
