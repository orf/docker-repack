#!/usr/bin/env sh
# Exit immediately if a command exits with a non-zero status.
set -e

# Treat unset variables as an error when substituting.
set -u

# Exit immediately if any command in a pipeline fails.
# Note: This is not POSIX-compliant, but works in many shells.
# shellcheck disable=SC3040
(set -o pipefail 2> /dev/null) && set -o pipefail

original_dir="$PWD"

cd / || exit;

combine()
{
  dest="$1"
  shift
  if [ -f "$dest" ]; then
    return
  fi
  for file in "$@"; do
    cat "$file" >> "$dest"
    rm -f "$file"
  done
}

completed() {
  cd "$original_dir" || exit;
  exec "$@"
}

