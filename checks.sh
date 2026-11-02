#!/usr/bin/env bash
# NOTE: deps update requires 103k lines for review... at some point
# cargo vet check --locked --frozen &&
cargo clippy --all-targets && cargo t --all-targets
