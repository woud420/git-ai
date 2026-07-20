# Bridge to the canonical Taskfile (https://taskfile.dev).
# Taskfile.yml remains the source of truth (CI invokes `task` directly);
# these targets exist for the standard `make check` / `make install` /
# `make test` entry points.
.PHONY: check install test dev build lint fmt doc

check:
	task lint
	task format:check
	task test

install:
	rustup show active-toolchain
	cargo fetch

test:
	task test

dev:
	task dev

build:
	task build

lint:
	task lint

fmt:
	task format

doc:
	task doc
