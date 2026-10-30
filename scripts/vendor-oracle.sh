#!/usr/bin/env bash
#
# Copies the test oracle of the crate into tests/oracle from libskybrush and
# from the specification of the Skybrush file formats, at the pinned commits.
#
# Usage: scripts/vendor-oracle.sh [--check]
#
#   --check   Compare tests/oracle with the pinned sources instead of
#             replacing it. Exits with a non-zero status if they differ.
#
# The sources are read from the sibling checkouts (../libskybrush and
# ../skybrush-file-formats-doc, overridden by LIBSKYBRUSH_REPO and SPEC_REPO)
# if they contain the pinned commits, and fetched from GitHub otherwise.
# Files are taken from the commits, never from the working trees, so
# uncommitted changes in the sibling checkouts do not reach the oracle.
#
# To update the oracle, change the pins below, run the script and review the
# differences in tests/oracle before committing them. libskybrush is pinned to
# a release tag and the commit it points to; between releases, the tag may be
# empty, and the commit alone is the pin.

set -euo pipefail

LIBSKYBRUSH_URL=https://github.com/skybrush-io/libskybrush.git
# The release that the crate is on par with
LIBSKYBRUSH_TAG=6.0.4
LIBSKYBRUSH_COMMIT=a6d4c4b8e4dafc804843c242499ff79603fdc3c7

SPEC_URL=https://github.com/skybrush-io/file-formats-doc.git
SPEC_COMMIT=710666990d718873a8d1d45e79f778fefe32f26f

ROOT=$(cd "$(dirname "$0")/.." && pwd)
ORACLE="$ROOT/tests/oracle"
LIBSKYBRUSH_REPO=${LIBSKYBRUSH_REPO:-"$ROOT/../libskybrush"}
SPEC_REPO=${SPEC_REPO:-"$ROOT/../skybrush-file-formats-doc"}

check=0
case "${1:-}" in
  "") ;;
  --check) check=1 ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

die() {
  echo "vendor-oracle: $*" >&2
  exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Prints the path of a git repository that contains the given commit: the
# local checkout if it has the commit, a fresh shallow fetch otherwise.
repo_with_commit() {
  local name=$1 local_repo=$2 url=$3 ref=$4 commit=$5
  if git -C "$local_repo" cat-file -e "$commit^{commit}" 2>/dev/null; then
    echo "$local_repo"
    return
  fi
  local repo="$work/$name.git"
  git init -q --bare "$repo"
  git -C "$repo" fetch -q --depth 1 "$url" "$ref" >&2 \
    || die "cannot fetch $ref from $url"
  echo "$repo"
}

# Fails unless the tag points to the pinned commit in the given repository.
verify_tag() {
  local repo=$1 tag=$2 commit=$3 actual
  if actual=$(git -C "$repo" rev-parse -q --verify "refs/tags/$tag^{commit}"); then
    [ "$actual" = "$commit" ] \
      || die "tag $tag of $repo points to $actual, not to the pinned $commit"
  fi
}

# Extracts a directory of a commit into a destination directory, without the
# leading path components.
extract() {
  local repo=$1 commit=$2 path=$3 dest=$4
  local depth
  depth=$(echo "$path" | awk -F/ '{ print NF }')
  mkdir -p "$dest"
  git -C "$repo" archive --format=tar "$commit" -- "$path" \
    | tar -x -C "$dest" --strip-components "$depth"
}

if [ -n "$LIBSKYBRUSH_TAG" ]; then
  libskybrush_ref="refs/tags/$LIBSKYBRUSH_TAG"
  libskybrush_pin="$LIBSKYBRUSH_COMMIT (tag $LIBSKYBRUSH_TAG)"
else
  libskybrush_ref="$LIBSKYBRUSH_COMMIT"
  libskybrush_pin="$LIBSKYBRUSH_COMMIT (no release)"
fi
libskybrush=$(repo_with_commit libskybrush "$LIBSKYBRUSH_REPO" \
  "$LIBSKYBRUSH_URL" "$libskybrush_ref" "$LIBSKYBRUSH_COMMIT")
spec=$(repo_with_commit spec "$SPEC_REPO" "$SPEC_URL" "$SPEC_COMMIT" "$SPEC_COMMIT")

if [ -n "$LIBSKYBRUSH_TAG" ]; then
  verify_tag "$libskybrush" "$LIBSKYBRUSH_TAG" "$LIBSKYBRUSH_COMMIT"
fi
git -C "$libskybrush" cat-file -e "$LIBSKYBRUSH_COMMIT^{commit}" 2>/dev/null \
  || die "$libskybrush_ref does not point to $LIBSKYBRUSH_COMMIT"

staging="$work/oracle"
extract "$spec" "$SPEC_COMMIT" spec/skyb/conformance "$staging/conformance"
extract "$spec" "$SPEC_COMMIT" spec/skyb/traces "$staging/format"
extract "$libskybrush" "$LIBSKYBRUSH_COMMIT" test/api_traces "$work/api"

# Only the traces and their documentation are part of the oracle; the C
# generators are not.
mkdir -p "$staging/api"
cp "$work/api"/*.json "$work/api/README.md" "$staging/api/"

# The per-fixture API traces are computed from test/fixtures/<name>.skyb of
# libskybrush. These are the same files as the show files of the format
# traces, so the API runner reads them from format/ instead of keeping a
# second copy. Fail if the two ever diverge.
for trace in "$staging/api"/*.json; do
  name=$(basename "$trace" .json)
  case "$name" in scenarios|tolerances) continue ;; esac
  fixture=$(git -C "$libskybrush" rev-parse -q --verify \
    "$LIBSKYBRUSH_COMMIT:test/fixtures/$name.skyb") \
    || die "no fixture for the API trace $name"
  show=$(git -C "$spec" rev-parse -q --verify \
    "$SPEC_COMMIT:spec/skyb/traces/$name.skyb") \
    || die "the API trace $name has no show file among the format traces"
  [ "$fixture" = "$show" ] \
    || die "the fixture of the API trace $name differs from format/$name.skyb"
done

cat > "$staging/SOURCES" <<EOF
# The test oracle of the crate, copied by scripts/vendor-oracle.sh from the
# commits below. Do not edit these files; change the pins in the script and
# run it again.
#
# directory     repository and commit, path
conformance/    $SPEC_URL $SPEC_COMMIT
                spec/skyb/conformance
format/         $SPEC_URL $SPEC_COMMIT
                spec/skyb/traces
api/            $LIBSKYBRUSH_URL $libskybrush_pin
                test/api_traces, without the generators
#
# The API traces of <name>.json are computed from test/fixtures/<name>.skyb of
# libskybrush, which is identical to format/<name>.skyb (checked by the script).
EOF

if [ "$check" = 1 ]; then
  [ -d "$ORACLE" ] || die "$ORACLE does not exist"
  if ! diff -r "$staging" "$ORACLE"; then
    die "tests/oracle differs from the pinned sources"
  fi
  echo "tests/oracle matches the pinned sources"
else
  rm -rf "$ORACLE"
  mkdir -p "$(dirname "$ORACLE")"
  mv "$staging" "$ORACLE"
  echo "copied the oracle to tests/oracle"
fi
