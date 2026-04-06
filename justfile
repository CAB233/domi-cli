build:
    cargo build

test: build
    mkdir ruleset || true
    cd ruleset && ../target/release/domi-cli --config ../config.example.toml

build-release:
    cargo build --release --locked

install-release: build-release
    install -Dvm755 target/release/domi-cli -t ~/.local/bin

clippy:
    cargo clippy

fmt:
    cargo fmt

check:
    cargo check

clean:
    cargo clean && rm ruleset/*.json

bump-patch:
    #!/usr/bin/env bash
    set -euo pipefail
    
    version=$(grep '^version = "' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    major=$(echo "$version" | cut -d. -f1)
    minor=$(echo "$version" | cut -d. -f2)
    patch=$(echo "$version" | cut -d. -f3)
    new_version="$major.$((minor)).$((patch + 1))"
    
    sed -i "s/version = \"$version\"/version = \"$new_version\"/" Cargo.toml
    
    cargo update -p domi-cli
    
    git add Cargo.toml Cargo.lock
    git commit -m "chore: bump version"
    git tag v"$new_version"
