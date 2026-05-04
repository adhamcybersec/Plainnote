#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Scan every tracked file in the repository (not just staged) for an
# SPDX-License-Identifier header. Used by CI to catch commits that
# bypassed the local pre-commit hook.

set -euo pipefail

SPDX_EXTENSIONS_REGEX='\.(rs|ts|tsx|js|mjs|cjs|svelte|css|html|sh|toml|yml|yaml|py)$'

EXEMPT_PATTERNS=(
    'apps/desktop/.svelte-kit/'
    'apps/desktop/build/'
    'apps/desktop/node_modules/'
    'apps/desktop/static/'
    'apps/desktop/src-tauri/target/'
    'apps/desktop/src-tauri/gen/'
    'apps/desktop/src-tauri/icons/'
    'apps/desktop/src-tauri/Cargo.lock'
    'apps/desktop/package.json'
    'apps/desktop/package-lock.json'
    'apps/desktop/tsconfig.json'
    'apps/desktop/src-tauri/Cargo.toml'
    'apps/desktop/src-tauri/tauri.conf.json'
    'apps/desktop/src-tauri/capabilities/default.json'
    'apps/desktop/src/app.d.ts'
    '.gitignore'
    'design/'
)

is_exempt() {
    local path="$1"
    for pattern in "${EXEMPT_PATTERNS[@]}"; do
        if [[ "$path" == *"$pattern"* ]] || [[ "$path" == "$pattern"* ]]; then
            return 0
        fi
    done
    return 1
}

mapfile -t tracked < <(git ls-files | grep -E "$SPDX_EXTENSIONS_REGEX")

missing=()
for f in "${tracked[@]}"; do
    [[ -f "$f" ]] || continue
    is_exempt "$f" && continue
    head -n 5 "$f" | grep -qF 'SPDX-License-Identifier:' || missing+=("$f")
done

if (( ${#missing[@]} > 0 )); then
    echo "::error::missing SPDX-License-Identifier in:"
    printf '    %s\n' "${missing[@]}"
    exit 1
fi
echo "✓ all tracked source files carry SPDX headers"
