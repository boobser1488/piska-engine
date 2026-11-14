#!/usr/bin/env bash
# Online check (CI only; it needs the network and is NOT part of the offline test-suite):
# every checksum pinned in packages.toml must be confirmed by its publisher.
#   zig_*      -> ziglang.org/download/index.json lists the same sha256 next to the same URL
#   android_*  -> developer.android.com/studio prints the same sha256
#   rustup_*   -> the publisher's .sha256 file is reachable and holds a hash
# Nothing is saved except the temporary pages; no installer code is run.
set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PISKA_INSTALL_NOMAIN=1
# shellcheck disable=SC1091
. "$HERE/../install.sh"
DATA=$(toml_flatten "$HERE/../packages.toml")
fail=0
index=$(curl_text "$(cfg zig.index_url)") || { echo "cannot fetch zig index.json"; exit 1; }
studio=$(curl_text "https://developer.android.com/studio") || { echo "cannot fetch the Android Studio page"; exit 1; }
for s in $(printf '%s\n' "$DATA" | awk -F'\t' '{ n = $1; sub(/\.[^.]*$/, "", n); print n }' | grep '^download\.' | sort -u); do
  pin=$(cfg "$s.sha256"); url=$(cfg "$s.url")
  case "$s" in
    download.zig_*)
      got=$(zig_index_sha "$index" "$url")
      if [ "$got" = "$pin" ]; then echo "ok   $s (index.json)"; else echo "FAIL $s: pinned $pin, index.json says '${got:-nothing}'"; fail=1; fi ;;
    download.android_*)
      case "$studio" in *"$pin"*) echo "ok   $s (developer.android.com)" ;; *) echo "FAIL $s: $pin is not printed on the Android Studio page (the page may have moved to a newer build)"; fail=1 ;; esac ;;
    download.rustup_*)
      if [ -n "$(sha_from_text "$(curl_text "$(cfg "$s.sha256_url")")")" ]; then echo "ok   $s (publisher .sha256 reachable)"; else echo "FAIL $s: no hash at $(cfg "$s.sha256_url")"; fail=1; fi ;;
    *) echo "FAIL $s: unknown download kind"; fail=1 ;;
  esac
done
exit $fail
