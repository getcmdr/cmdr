#!/usr/bin/env bash
# The slow-checks fixer's claim on its issue, so it never works on something another agent (or David) already took, and
# stands down when someone takes it mid-run. The claim is the issue's card on the "Cmdr backlog" board sitting in
# Implementing, the same signal every agent uses (docs/tooling/backlog-board.md), plus one comment saying so.
#
#   claim.sh free <issue> <since>    exit 0 if nobody else is on it; else exit 1 and print why
#   claim.sh take <issue> <run id>   move the card to Implementing, comment, print the claim time (ISO, UTC)
#   claim.sh held <issue> <since>    exit 0 while the claim is still ours; else exit 1 and print why
#   claim.sh release <issue> <status> move the card to <status> (Todo, say), if it's still in Implementing
#
# "Someone else" is the card in Implementing, or a comment newer than <since> by a maintainer (`OWNER`, `MEMBER`, or
# `COLLABORATOR`) who isn't one of the two bots. Only the comment's author and time are read, never its text: the repo
# is public, so comment text is untrusted input and never reaches the agent (CLAUDE.md § Must-knows). Strangers can't
# block it either, since their comments carry another author association.
#
# Needs `gh` signed in as the Cmdr agents App with Organization > Projects read-write; any lookup that fails exits 2,
# and the callers treat that as "not free" (fail closed).
set -euo pipefail

repo_owner="getcmdr"
repo_name="cmdr"
project_number=1
claimed_status="Implementing"
bots='["github-actions[bot]", "cmdr-m1-agents[bot]"]'

die() { echo "claim.sh: $*" >&2; exit 2; }

# The issue's card on the board: its item id and its Status ("" when it's on the board with no status, "none" when
# it isn't on the board at all).
card() {
  gh api graphql -f query='
    query($owner: String!, $repo: String!, $n: Int!) {
      repository(owner: $owner, name: $repo) {
        issue(number: $n) {
          id
          projectItems(first: 20) {
            nodes {
              id
              project { number owner { ... on Organization { login } } }
              status: fieldValueByName(name: "Status") { ... on ProjectV2ItemFieldSingleSelectValue { name } }
            }
          }
        }
      }
    }' -f owner="$repo_owner" -f repo="$repo_name" -F n="$1" \
    --jq "[.data.repository.issue.projectItems.nodes[]
      | select(.project.number == $project_number and .project.owner.login == \"$repo_owner\")] | first
      | if . == null then \"none\t-\" else \"\(.status.name // \"\")\t\(.id)\" end" ||
    die "couldn't read #$1's card (does the App have Projects read-write?)"
}

# A maintainer's comment on the issue newer than $2, by anyone but the bots: prints "<author> at <time>", or nothing.
maintainer_comment_since() {
  gh api "repos/$repo_owner/$repo_name/issues/$1/comments?per_page=100&since=$2" \
    --jq "[.[] | select(.created_at > \"$2\")
      | select(.author_association == \"OWNER\" or .author_association == \"MEMBER\" or .author_association == \"COLLABORATOR\")
      | select(.user.login as \$u | $bots | any(.[]; . == \$u) | not)] | last
      | if . == null then empty else \"\(.user.login) commented at \(.created_at)\" end" ||
    die "couldn't read #$1's comments"
}

# Sets the card's Status by option name, adding the issue to the board first when it isn't on it.
set_status() {
  local issue="$1" status="$2" item project_id field_id option_id
  read -r project_id field_id option_id < <(gh api graphql -f query='
    query($owner: String!, $n: Int!) {
      organization(login: $owner) {
        projectV2(number: $n) {
          id
          field(name: "Status") { ... on ProjectV2SingleSelectField { id options { id name } } }
        }
      }
    }' -f owner="$repo_owner" -F n="$project_number" \
    --jq ".data.organization.projectV2 | \"\(.id) \(.field.id) \(.field.options[] | select(.name == \"$status\") | .id)\"") ||
    die "couldn't read the board's Status field"
  [[ -n "${option_id:-}" ]] || die "the board has no \"$status\" status"
  item="$(card "$issue" | cut -f2)"
  if [[ "$item" == "-" ]]; then
    item="$(gh project item-add "$project_number" --owner "$repo_owner" \
      --url "https://github.com/$repo_owner/$repo_name/issues/$issue" --format json --jq .id)" ||
      die "couldn't add #$issue to the board"
  fi
  gh project item-edit --project-id "$project_id" --id "$item" --field-id "$field_id" \
    --single-select-option-id "$option_id" >/dev/null || die "couldn't move #$issue's card to $status"
}

cmd="${1:-}"
issue="${2:-}"
[[ "$issue" =~ ^[0-9]+$ ]] || die "usage: claim.sh free|take|held|release <issue> <arg>"

case "$cmd" in
  free)
    since="${3:?claim.sh free needs <since>}"
    status="$(card "$issue" | cut -f1)"
    if [[ "$status" == "$claimed_status" ]]; then
      echo "#$issue's card is already in $claimed_status"
      exit 1
    fi
    who="$(maintainer_comment_since "$issue" "$since")"
    if [[ -n "$who" ]]; then
      echo "#$issue has a newer maintainer comment ($who)"
      exit 1
    fi
    ;;
  take)
    run_id="${3:?claim.sh take needs <run id>}"
    set_status "$issue" "$claimed_status"
    claimed_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    gh issue comment "$issue" -R "$repo_owner/$repo_name" --body "The slow-checks fixer is working on red run \
https://github.com/$repo_owner/$repo_name/actions/runs/$run_id and has moved this card to $claimed_status. Its notes \
will be in the issue body. If you take this over, comment here or move the card out of $claimed_status, and it stands \
down." >/dev/null || die "couldn't comment on #$issue"
    echo "$claimed_at"
    ;;
  held)
    since="${3:?claim.sh held needs <since>}"
    status="$(card "$issue" | cut -f1)"
    if [[ "$status" != "$claimed_status" ]]; then
      echo "#$issue's card moved to ${status:-no status}"
      exit 1
    fi
    who="$(maintainer_comment_since "$issue" "$since")"
    if [[ -n "$who" ]]; then
      echo "#$issue has a maintainer comment since the claim ($who)"
      exit 1
    fi
    ;;
  release)
    to="${3:?claim.sh release needs <status>}"
    if [[ "$(card "$issue" | cut -f1)" == "$claimed_status" ]]; then
      set_status "$issue" "$to"
    fi
    ;;
  *) die "unknown command: $cmd" ;;
esac
