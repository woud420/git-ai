#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"
cargo test metrics::model_pricing::tests::regenerate_models_dev_pricing_snapshot -- --ignored --exact --nocapture
git diff -- src/metrics/models_dev_pricing_snapshot.json
