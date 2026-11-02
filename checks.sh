#!/usr/bin/env bash
cargo vet check --locked --frozen && cargo clippy --all-targets && cargo t --all-targets
