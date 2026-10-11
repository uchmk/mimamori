#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
#
# Create or correct the issue labels the lanes and sessions use (LABELS in
# scripts/merge-lanes.py): a missing one is created, one whose colour or
# description differs is corrected, any other label is left alone. Running it
# again changes nothing. The Merge lanes workflow does the same every hour;
# this is for a new repository or to see the result at once.
#
#   scripts/labels.sh                       # the token from gh, the repository from origin
#   GITHUB_TOKEN=… GITHUB_REPOSITORY=uchmk/kura scripts/labels.sh
#   MERGE_LANES_DRY_RUN=1 scripts/labels.sh # say what would change

set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${GITHUB_TOKEN:-}" ]; then
    GITHUB_TOKEN=${GH_TOKEN:-$(gh auth token)}
fi
if [ -z "${GITHUB_REPOSITORY:-}" ]; then
    # https://github.com/uchmk/kura(.git), git@github.com:uchmk/kura.git or a proxy URL ending in the same
    GITHUB_REPOSITORY=$(git remote get-url origin | sed -E 's#\.git$##; s#.*[:/]([^/:]+/[^/]+)$#\1#')
fi
export GITHUB_TOKEN GITHUB_REPOSITORY
exec python3 scripts/merge-lanes.py --labels
