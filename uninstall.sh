#!/bin/bash
# git-ai uninstaller
#
# Primary path: delegates to `git-ai uninstall "$@"` (the installed binary).
# Standalone fallback: when the binary is already gone, removes the known
# artifacts directly and prints a list of anything that could not be removed
# automatically.
#
# Usage:
#   ./uninstall.sh            # interactive (prompts for confirmation)
#   ./uninstall.sh --yes      # skip confirmation
#   ./uninstall.sh --yes --purge  # also delete ~/.git-ai

set -euo pipefail

# Bail early if HOME is unset — all artifact paths are anchored there.
if [ -z "${HOME:-}" ]; then
    echo "Error: HOME is not set. Cannot determine artifact locations." >&2
    exit 1
fi

# ── colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

info()    { echo -e "${GREEN}$*${NC}"; }
warn()    { echo -e "${YELLOW}Warning: $*${NC}" >&2; }
failure() { echo -e "${RED}Error: $*${NC}" >&2; }

# ── locate the git-ai binary ─────────────────────────────────────────────────
GITAI_BIN=""
for candidate in \
    "${HOME:-}/.git-ai/bin/git-ai" \
    "${HOME:-}/.local/bin/git-ai" \
    "$(command -v git-ai 2>/dev/null || true)"
do
    if [ -x "$candidate" ]; then
        GITAI_BIN="$candidate"
        break
    fi
done

# ── primary path: delegate to the installed binary ────────────────────────────
if [ -n "$GITAI_BIN" ]; then
    exec "$GITAI_BIN" uninstall "$@"
fi

# ── standalone fallback ───────────────────────────────────────────────────────
# The binary is already gone.  Do a best-effort removal of the well-known
# artifacts that install.sh creates and report what remains.

warn "git-ai binary not found — running standalone fallback removal."
echo ""

PURGE=false
ASSUME_YES=false
for arg in "$@"; do
    case "$arg" in
        --purge) PURGE=true ;;
        --yes|-y) ASSUME_YES=true ;;
    esac
done

if [ "$ASSUME_YES" != "true" ]; then
    if [ "$PURGE" = "true" ]; then
        echo "This will remove git-ai artifacts AND delete ~/.git-ai."
    else
        echo "This will remove git-ai artifacts (binary dir, symlink, shell rc edits)."
    fi
    printf 'Continue? [y/N] '
    read -r answer
    case "$answer" in
        y|Y|yes|YES) ;;
        *) echo "Aborted."; exit 0 ;;
    esac
fi

REMAINING=""

# 1. Remove binary directory
BIN_DIR="$HOME/.git-ai/bin"
if [ -d "$BIN_DIR" ]; then
    rm -rf "$BIN_DIR" && info "Removed $BIN_DIR" || { warn "Could not remove $BIN_DIR"; REMAINING="$REMAINING $BIN_DIR"; }
fi

# 2. Remove ~/.local/bin/git-ai symlink (only when it pointed at git-ai)
LOCAL_LINK="$HOME/.local/bin/git-ai"
if [ -L "$LOCAL_LINK" ]; then
    target=$(readlink "$LOCAL_LINK" 2>/dev/null || true)
    case "$target" in
        */.git-ai/*) rm -f "$LOCAL_LINK" && info "Removed symlink $LOCAL_LINK" || warn "Could not remove $LOCAL_LINK" ;;
    esac
fi

# 3. Remove fenced PATH blocks from shell rc files.
# Fence markers: # >>> git-ai >>> ... # <<< git-ai <<<
# Also strips bare legacy marker lines (# Added by git-ai installer ...).
FENCE_OPEN="# >>> git-ai >>>"
FENCE_CLOSE="# <<< git-ai <<<"
LEGACY_MARKER="# Added by git-ai installer"

for rc_file in \
    "$HOME/.bashrc" \
    "$HOME/.bash_profile" \
    "$HOME/.zshrc" \
    "$HOME/.config/fish/config.fish"
do
    [ -f "$rc_file" ] || continue

    # Only attempt fence removal when BOTH markers are present.  This prevents
    # the open-without-close case from truncating everything after the open marker.
    # (Issue: 'close' is a gawk builtin — renamed to fence_open/fence_close to
    # avoid 'fatal: cannot use gawk builtin close as variable name' on Linux.)
    if grep -qxF "$FENCE_OPEN" "$rc_file" && grep -qxF "$FENCE_CLOSE" "$rc_file"; then
        # Write via temp file + mv to preserve exact trailing-newline count and
        # avoid partial writes on failure.
        tmp_file="${rc_file}.gitai_tmp"
        awk \
            -v fence_open="$FENCE_OPEN" \
            -v fence_close="$FENCE_CLOSE" \
            'BEGIN { inside=0 }
            $0 == fence_open  { inside=1; next }
            $0 == fence_close { inside=0; next }
            inside { next }
            { print }' "$rc_file" > "$tmp_file" && mv -f "$tmp_file" "$rc_file"
        info "Cleaned git-ai fence block from $rc_file"
    elif grep -qxF "$FENCE_OPEN" "$rc_file" || grep -qxF "$FENCE_CLOSE" "$rc_file"; then
        # One marker present without the other — do not touch the file.
        warn "Unbalanced fence markers in $rc_file — left untouched (please review manually)"
    else
        # No fence block — strip bare legacy installer lines if present.
        if grep -qF "$LEGACY_MARKER" "$rc_file" || grep -qF "/.git-ai/bin" "$rc_file"; then
            tmp_file="${rc_file}.gitai_tmp"
            awk \
                -v legacy="$LEGACY_MARKER" \
                'index($0, legacy) > 0 { next }
                index($0, "/.git-ai/bin") > 0 { next }
                { print }' "$rc_file" > "$tmp_file" && mv -f "$tmp_file" "$rc_file"
            info "Cleaned legacy git-ai lines from $rc_file"
        fi
    fi
done

# 4. Revert global git trace2 config (only when it points at git-ai)
if command -v git >/dev/null 2>&1; then
    target=$(git config --global --get trace2.eventTarget 2>/dev/null || true)
    case "$target" in
        */.git-ai/*)
            git config --global --remove-section trace2 2>/dev/null && \
                info "Removed git trace2 config" || \
                warn "Could not remove git trace2 config — remove manually with: git config --global --remove-section trace2"
            ;;
        "")  ;;
        *)   info "trace2.eventTarget ($target) does not point at git-ai — left untouched" ;;
    esac
fi

# 5. Optionally remove ~/.git-ai data directory
if [ "$PURGE" = "true" ]; then
    DATA_DIR="$HOME/.git-ai"
    if [ -d "$DATA_DIR" ]; then
        rm -rf "$DATA_DIR" && info "Removed $DATA_DIR" || { warn "Could not remove $DATA_DIR"; REMAINING="$REMAINING $DATA_DIR"; }
    fi
fi

echo ""
if [ -n "$REMAINING" ]; then
    warn "Some artifacts could not be removed automatically:"
    for item in $REMAINING; do
        echo "  $item"
    done
    echo ""
    echo "Please remove these manually."
fi

info "git-ai uninstall complete."
echo "Note: repo-local .git/ai directories are not tracked and were not removed."
if [ "$PURGE" != "true" ]; then
    echo "Your config and local attribution data in ~/.git-ai are kept."
    echo "Remove with: $0 --yes --purge"
fi
