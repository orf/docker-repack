#!/usr/bin/env sh

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
