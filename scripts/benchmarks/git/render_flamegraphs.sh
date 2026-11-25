#!/usr/bin/env bash
set -euo pipefail

PROFILE_ROOT="${1:?usage: render_flamegraphs.sh <profile-root> <output-root>}"
OUTPUT_ROOT="${2:?usage: render_flamegraphs.sh <profile-root> <output-root>}"

if ! command -v perf >/dev/null 2>&1; then
  echo "error: perf is required to render flamegraphs" >&2
  exit 1
fi
if ! command -v inferno-collapse-perf >/dev/null 2>&1; then
  echo "error: inferno-collapse-perf is required to render flamegraphs" >&2
  exit 1
fi
if ! command -v inferno-flamegraph >/dev/null 2>&1; then
  echo "error: inferno-flamegraph is required to render flamegraphs" >&2
  exit 1
fi
if ! command -v inferno-diff-folded >/dev/null 2>&1; then
  echo "error: inferno-diff-folded is required to compare flamegraphs" >&2
  exit 1
fi

mkdir -p "$OUTPUT_ROOT"

render_variant() {
  local variant="$1"
  local variant_root="$PROFILE_ROOT/$variant"
  local perf_script="$OUTPUT_ROOT/${variant}.perf"
  local folded="$OUTPUT_ROOT/${variant}.folded"
  local flamegraph="$OUTPUT_ROOT/${variant}.svg"
  local data_file
  if [[ ! -d "$variant_root" ]]; then
    echo "error: no profiles found for $variant" >&2
    return 1
  fi

  : > "$perf_script"

  while IFS= read -r -d '' data_file; do
    if ! perf script -i "$data_file" >> "$perf_script"; then
      echo "error: failed to read perf profile $data_file" >&2
      return 1
    fi
  done < <(find "$variant_root" -type f -name '*.data' -print0 | sort -z)

  if [[ ! -s "$perf_script" ]]; then
    echo "error: no perf samples found for $variant" >&2
    return 1
  fi

  inferno-collapse-perf < "$perf_script" > "$folded"
  inferno-flamegraph --title "git-ai $variant daemon profile" < "$folded" > "$flamegraph"
  echo "wrote $flamegraph"
}

for variant in main_daemon current_daemon; do
  render_variant "$variant"
done

main_folded="$OUTPUT_ROOT/main_daemon.folded"
current_folded="$OUTPUT_ROOT/current_daemon.folded"

# Frame widths follow current; red frames take a larger share than on main.
inferno-diff-folded --normalize "$main_folded" "$current_folded" \
  | inferno-flamegraph --title "git-ai daemon: current vs main (red = more time on current)" \
  > "$OUTPUT_ROOT/diff_current_vs_main.svg"
# Frame widths follow main, so frames that disappeared on current still show;
# blue frames take a smaller share on current.
inferno-diff-folded --normalize "$current_folded" "$main_folded" \
  | inferno-flamegraph --negate --title "git-ai daemon: main vs current (blue = less time on current)" \
  > "$OUTPUT_ROOT/diff_main_vs_current.svg"
echo "wrote $OUTPUT_ROOT/diff_current_vs_main.svg and $OUTPUT_ROOT/diff_main_vs_current.svg"

python3 "$(dirname "${BASH_SOURCE[0]}")/compare_flamegraph_profiles.py" \
  "$main_folded" "$current_folded" --output "$OUTPUT_ROOT/comparison.md"
echo "wrote $OUTPUT_ROOT/comparison.md"
