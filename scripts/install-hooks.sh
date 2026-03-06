#!/usr/bin/env bash
# Installs the project's git hooks from scripts/hooks/ into .git/hooks/.
# Run once after cloning: bash scripts/install-hooks.sh
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_SRC="$REPO_ROOT/scripts/hooks"
HOOKS_DST="$REPO_ROOT/.git/hooks"

for hook in "$HOOKS_SRC"/*; do
    name="$(basename "$hook")"
    dst="$HOOKS_DST/$name"
    cp "$hook" "$dst"
    chmod +x "$dst"
    echo "installed: .git/hooks/$name"
done

echo "done."

