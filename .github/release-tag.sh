#!/bin/sh
# release.yml tag job (taskq.md § 8 item 1): prints `tag=v<version>` for GITHUB_OUTPUT when this run must release.
# Tag push: that tag. Live push: the next unused vX.Y.Z, the greater of the Cargo.toml version and the highest
# vX.Y.Z tag plus one patch, created here on the pushed commit; Cargo.toml is never bumped by a commit.
# A pushed commit that already has a tag reuses it while its release is absent or a draft (rerun), else releases nothing.
# An unreadable or invalid version or tag list fails without a tag. An existing tag never moves.
set -eu
if [ "$GITHUB_REF_TYPE" = tag ]; then echo "tag=$GITHUB_REF_NAME"; exit 0; fi
fail() { echo "::error::$1; no tag" >&2; exit 1; }
# A pending automatic run may arrive after a newer source was released.
if [ "$GITHUB_REF_NAME" = main ]; then
  latest=$(gh api "repos/$GITHUB_REPOSITORY/releases/latest" --jq .tag_name) || fail "cannot read latest release"
  coverage=$(gh api "repos/$GITHUB_REPOSITORY/compare/$GITHUB_SHA...$latest" --jq .status) || fail "cannot read released ancestry"
  case "$coverage" in
    ahead|identical) echo "$GITHUB_SHA is already included in $latest; no release" >&2; exit 0;;
    behind) ;;
    *) fail "automatic source is not a successor of $latest";;
  esac
fi
cargo=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' Cargo.toml | head -1)
[ -n "$cargo" ] || fail "no X.Y.Z version in Cargo.toml"
refs=$(gh api --paginate "repos/$GITHUB_REPOSITORY/git/matching-refs/tags/v" --jq '.[] | "\(.ref) \(.object.sha)"') ||
  fail "cannot read tags"
own=$(printf '%s\n' "$refs" | awk -v s="$GITHUB_SHA" '$2 == s { sub("refs/tags/", "", $1); print $1; exit }')
if [ -n "$own" ]; then
  if [ "$(gh release view "$own" --repo "$GITHUB_REPOSITORY" --json isDraft --jq .isDraft 2>/dev/null)" = false ]; then
    echo "$GITHUB_SHA is $own, released; no release" >&2
  else echo "tag=$own"; fi
  exit 0
fi
top=$(printf '%s\n' "$refs" | sed -n 's|^refs/tags/v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\) .*|\1|p' | sort -V | tail -1)
next=$cargo
if [ -n "$top" ]; then
  bumped=$(echo "$top" | awk -F. '{ print $1 "." $2 "." $3 + 1 }')
  [ "$(printf '%s\n%s\n' "$cargo" "$bumped" | sort -V | tail -1)" = "$cargo" ] || next=$bumped
fi
gh api "repos/$GITHUB_REPOSITORY/git/refs" -f ref="refs/tags/v$next" -f sha="$GITHUB_SHA" > /dev/null ||
  fail "cannot create v$next"
echo "tag=v$next"
