#!/usr/bin/env bash
# Fetches the external SVG corpora used for cross-renderer import comparison.
#
# The corpora are not vendored: they are large and separately licensed. This clones them into
# `target/svg-test-corpus/`, which is gitignored, so a clean checkout has no trace of them.
#
# Usage:
#   ./tools/svg-import-tests/fetch-corpus.sh                 # resvg test suite (default)
#   ./tools/svg-import-tests/fetch-corpus.sh --pin <sha>     # fetch a specific commit
#
# Afterwards, run the comparison over the corpus:
#   cargo run -p svg-import-tests -- --dir target/svg-test-corpus/resvg-test-suite/tests --recursive --skip-text --skip-filters
set -euo pipefail

CORPUS_DIR="${CORPUS_DIR:-target/svg-test-corpus}"
RSVG_REPO="https://github.com/linebender/resvg-test-suite.git"
RSVG_DIR="$CORPUS_DIR/resvg-test-suite"
PIN=""

while [ $# -gt 0 ]; do
	case "$1" in
	--pin)
		PIN="$2"
		shift 2
		;;
	*)
		echo "Unknown argument: $1" >&2
		exit 1
		;;
	esac
done

if [ -d "$RSVG_DIR/.git" ]; then
	echo "resvg test suite already present at $RSVG_DIR"
else
	mkdir -p "$CORPUS_DIR"
	echo "Cloning resvg test suite into $RSVG_DIR ..."
	if [ -n "$PIN" ]; then
		git clone --filter=blob:none --no-checkout "$RSVG_REPO" "$RSVG_DIR"
		git -C "$RSVG_DIR" checkout "$PIN"
	else
		git clone --depth 1 "$RSVG_REPO" "$RSVG_DIR"
	fi
fi

count=$(find "$RSVG_DIR/tests" -name '*.svg' | wc -l)
echo
echo "resvg test suite: $count SVG files under $RSVG_DIR/tests"
echo
echo "Run the comparison with:"
echo "  cargo run -p svg-import-tests -- --dir $RSVG_DIR/tests --recursive --skip-text --skip-filters"
echo
echo "Text and filters are skipped by default because pixel-matching them is unreliable across"
echo "font versions and rasterizer backends. Drop the flags to include them and see the raw numbers."
