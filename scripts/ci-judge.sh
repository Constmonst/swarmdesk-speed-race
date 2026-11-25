#!/usr/bin/env bash
# Judges one pull request. The deciding workflow runs this from a checkout of the BASE
# branch, so every line here is reviewed code. The only thing taken from the pull request
# is the bytes of one file, fetched by content hash. The fork is never checked out.
#
# Always leaves "$OUT/final.json" behind: a verdict, a reason and what was judged.
set -euo pipefail
: "${PR:?}" "${REPO:?}" "${DEFAULT_BRANCH:?}" "${GH_TOKEN:?}" "${OUT:?}" "${RUN_URL:?}"
API=https://api.github.com
PROBLEM="${PROBLEM:-speed-race}"
mkdir -p "$OUT/api"

printf '%s' "$PR" | grep -Eqx '[1-9][0-9]{0,6}' || { echo "bad pull request number"; exit 1; }

# write <verdict> <reason> [json object with more fields]
write() {
  local code="fault"
  [ "$1" = "REJECTED" ] && code="guard_rejected"
  jq -n --arg verdict "$1" --arg reason "$2" --arg reason_code "$code" --argjson pr "$PR" --arg run_url "$RUN_URL" \
    --argjson more "${3:-{\}}" '$more + {verdict: $verdict, reason: $reason, reason_code: ($more.reason_code // $reason_code), pr: $pr, run_url: $run_url}' \
    > "$OUT/final.json"
  echo "$1 ($2)"
}
write FAULT "the check stopped before it reached a verdict"

get() { # <api path> <file> [accept]
  curl -sSf --proto '=https' --max-redirs 0 --max-filesize 20000000 \
    -H "Authorization: Bearer $GH_TOKEN" -H "Accept: ${3:-application/vnd.github+json}" \
    -H "X-GitHub-Api-Version: 2022-11-28" -o "$2" "$API/repos/$REPO/$1"
}

get "pulls/$PR" "$OUT/api/pr.json"
get "pulls/$PR/files?per_page=100" "$OUT/api/files.json"
head=$(jq -r '.head.sha // ""' "$OUT/api/pr.json")
printf '%s' "$head" | grep -Eqx '[0-9a-f]{40}' || { write REJECTED "the pull request has no head commit"; exit 0; }
get "git/trees/$head?recursive=1" "$OUT/api/tree.json"
get "pulls/$PR/commits?per_page=5" "$OUT/api/commits.json"

# One place decides what a pull request may be: exactly one record holder file, replaced.
if ! guard=$(node .github/scripts/pr-guard.mjs pr "$OUT/api/pr.json" "$OUT/api/files.json" \
  "$OUT/api/tree.json" "$OUT/api/commits.json" "$REPO" "$DEFAULT_BRANCH"); then
  write REJECTED "$(jq -r '.refused // "the pull request was refused"' <<<"$guard")"
  exit 0
fi
target=$(jq -r .target <<<"$guard")
step=$(jq -r .step <<<"$guard")
blob=$(jq -r .blob <<<"$guard")
author=$(jq -r .author <<<"$guard")
# The record holder this run compares against, by the content hash of the very file the
# checker will read. The report job writes the record only if main still holds this file.
holder_blob=$(git hash-object "targets/$target/holder.rs")
meta=$(jq -c --arg holder "$holder_blob" \
  '{target, step, head_sha: .head, author, candidate_blob: .blob, holder_blob: $holder}' <<<"$guard")

# One author may not fill the queue, and one pull request is not judged for ever. Both counts
# depend only on what this author did, so nobody else can use them to block a pull request:
# the author's own open pull requests, and the verdicts this workflow posted here. The verdict
# count reads the newest 1000 comments at most, so comments written before the verdicts do not
# hide them. It is not refused beyond that on purpose: a refusal would let anyone block a
# pull request by commenting on it. The count keeps one pull request from being judged over
# and over; it is not what makes a verdict sound.
curl -sSf --proto '=https' --max-redirs 0 --max-filesize 2000000 \
  -H "Authorization: Bearer $GH_TOKEN" -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" -o "$OUT/api/mine.json" \
  "$API/search/issues?q=repo:$REPO+is:pr+is:open+author:$author&per_page=1"
total=$(jq -r '.comments // 0 | if type == "number" then . else 0 end' "$OUT/api/pr.json")
last=$(( (total + 99) / 100 ))
[ "$last" -ge 1 ] || last=1
first=$(( last > 10 ? last - 9 : 1 ))
pages=()
for page in $(seq "$first" "$last"); do
  get "issues/$PR/comments?per_page=100&page=$page" "$OUT/api/comments-$page.json"
  pages+=("$OUT/api/comments-$page.json")
done
jq -s add "${pages[@]}" > "$OUT/api/comments.json"
if ! limits=$(node .github/scripts/pr-guard.mjs limits "$OUT/api/mine.json" "$OUT/api/comments.json"); then
  write REJECTED "$(jq -r '.refused // "the pull request was refused"' <<<"$limits")" "$meta"
  exit 0
fi

# The step must be open at the desk. Without a desk address the repository judges on its own.
if [ -n "${SWARMDESK_URL:-}" ]; then
  printf '%s' "$SWARMDESK_URL" | grep -Eqx 'https://[a-z0-9.-]+' || { write FAULT "bad desk address" "$meta"; exit 0; }
  if ! curl -sSf --proto '=https' --max-time 20 -o "$OUT/api/steps.json" "$SWARMDESK_URL/p/$PROBLEM/steps.json"; then
    write FAULT "the desk could not be reached" "$meta"
    exit 0
  fi
  if ! open=$(node .github/scripts/pr-guard.mjs step "$OUT/api/steps.json" "$step" "$target"); then
    write REJECTED "$(jq -r '.refused // "the step was refused"' <<<"$open")" "$meta"
    exit 0
  fi
fi

# The content arrives by its hash, as bytes, into a file this script creates.
get "git/blobs/$blob" "$OUT/candidate.rs" "application/vnd.github.raw"
# The bytes that are judged are the bytes the report job will write as the record.
if [ "$(git hash-object "$OUT/candidate.rs")" != "$blob" ]; then
  write FAULT "the file that arrived is not the file of the pull request" "$meta"
  exit 0
fi

# The site sends the record holder's share of the reference's fuel; it tapers the margin.
share_args=()
if [ -s "$OUT/api/steps.json" ]; then
  share=$(jq -r --arg t "$target" '(.steps // [] | map(select(.target == $t)) | .[0].holder_share_bps // "") | tostring' "$OUT/api/steps.json" 2>/dev/null || true)
  if printf '%s' "$share" | grep -Eqx '[0-9]{1,5}' && [ "$share" -le 10000 ]; then
    share_args=(--holder-share-bps "$share")
  fi
fi

cargo build --release --locked --quiet
set +e
# Six sets of fresh inputs: the first decides nothing alone. The candidate must answer all of
# them correctly, and the margin is taken over all six together.
env -u GH_TOKEN ./target/release/sdcheck judge --target "$target" --confirm 5 \
  --candidate "$OUT/candidate.rs" --holder "targets/$target/holder.rs" \
  "${share_args[@]}" --json "$OUT/verdict.json"
set -e
if [ ! -s "$OUT/verdict.json" ]; then
  write FAULT "the checker produced no verdict" "$meta"
  exit 0
fi
jq -c --argjson meta "$meta" --argjson pr "$PR" --arg run_url "$RUN_URL" \
  '. + $meta + {pr: $pr, run_url: $run_url}' "$OUT/verdict.json" > "$OUT/final.json"
