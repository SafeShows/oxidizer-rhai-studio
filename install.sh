#!/bin/sh
# Builds the mod and copies it into the game's mods folder (Linux, macOS). Usage: ./install.sh [mods folder]
set -e
id=$(sed -n 's/^name = "\([^"]*\)".*/\1/p' Cargo.toml | head -1)
mods="$1"
if [ -z "$mods" ]; then
  for d in "$HOME/Documents/My Games/Kitten Space Agency" "$HOME/My Games/Kitten Space Agency"; do
    if [ -d "$d" ]; then mods="$d/mods"; break; fi
  done
fi
[ -n "$mods" ] || { echo "The game's folder wasn't found; pass the mods folder: ./install.sh <mods folder>"; exit 1; }
cargo build --release
case "$(uname)" in Darwin) lib="lib$id.dylib" ;; *) lib="lib$id.so" ;; esac
mkdir -p "$mods/$id/bin"
cp -R mod/. "$mods/$id/"
cp "target/release/$lib" "$mods/$id/bin/"
echo "Installed to $mods/$id"
