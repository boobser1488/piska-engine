#!/usr/bin/env bash
# Piska Engine installer for Linux and macOS: installs and configures everything a developer needs.
#
#   install.sh [--components core,cpp,web,android,linux-cross,engine|all] [--yes] [--dry-run]
#              [--prefix DIR] [--cache DIR] [--android-sdk DIR] [--accept-licenses]
#              [--resume | --repair | --uninstall] [--no-prefs] [--file-associations]
#              [--cargo-target-dir DIR] [--list-components] [--base-url URL]
#              [--json] [--accept-license ID[,ID]] [--confirm-elevation] [--skip-steps ID[,ID]]
#              [--source DIR] [--elevate pkexec|sudo|none] [--no-shortcuts] [--desktop-shortcut]
#
# --json: machine-readable output for the graphical installer, one JSON object per line on stdout
# (hello, plan, step, progress, log, prompt, licence, elevation, reboot, uninstall-plan, error, summary);
# questions are "prompt" events answered with one line on stdin ("yes" / "no").
#
# Versions, URLs, checksums and package lists come from install/packages.toml (one source of
# truth, shared with install.ps1, `piska-export doctor` and docs/install.md).
# --dry-run prints every command, download, checksum and environment change and touches nothing.
# Needs bash 3.2+ (the macOS default), awk, curl, tar, unzip, and sha256sum or shasum.

set -u
set -o pipefail 2>/dev/null || true

EXIT_FAILED=1; EXIT_BAD_ARGS=2; EXIT_REBOOT=3; EXIT_DECLINED=4
ALL_COMPONENTS="core cpp web android linux-cross engine"
# Steps whose absence does not break the steps after them: --skip-steps may skip only these.
SKIPPABLE_STEPS="core.rust-components core.vulkan engine.desktop engine.assoc engine.uninstaller config.preferences config.cargo"
JSON=0; CURRENT_STEP=""; STAT_TOTAL=0; STAT_DONE=0; STAT_SKIPPED=0; STAT_FAILED=0; FAILED_STEP=""; FAILED_ERROR=""; MANUAL_COMMAND=""; REBOOT_NEEDED=0

# ---------------------------------------------------------------------------------------
# Data file: a subset of TOML (see packages.toml). cfg section.key -> value; arrays are space separated.
# ---------------------------------------------------------------------------------------

toml_flatten() {
  awk '
    /^[ \t]*#/ || /^[ \t]*$/ { next }
    /^[ \t]*\[.*\][ \t]*$/ { s = $0; gsub(/^[ \t]*\[|\][ \t]*$/, "", s); sec = s; next }
    {
      i = index($0, "="); if (i == 0) next
      k = substr($0, 1, i - 1); v = substr($0, i + 1)
      gsub(/^[ \t]+|[ \t]+$/, "", k); gsub(/^[ \t]+|[ \t]+$/, "", v)
      if (v ~ /^\[/) {
        out = ""
        while (match(v, /"[^"]*"/)) { out = out (out == "" ? "" : " ") substr(v, RSTART + 1, RLENGTH - 2); v = substr(v, RSTART + RLENGTH) }
        v = out
      } else if (v ~ /^"/) { sub(/^"/, "", v); sub(/"[ \t]*$/, "", v) }
      print sec "." k "\t" v
    }' "$1"
}

cfg() { printf '%s\n' "$DATA" | awk -F'\t' -v k="$1" '$1 == k { print $2; exit }'; }

# ---------------------------------------------------------------------------------------
# Pure helpers (unit-tested by install/tests/run.sh)
# ---------------------------------------------------------------------------------------

# version_ge FOUND MIN -> success when FOUND >= MIN (numeric, dot separated; missing parts are 0)
version_ge() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    na = split(a, x, "."); nb = split(b, y, "."); n = (na > nb ? na : nb)
    for (i = 1; i <= n; i++) { p = x[i] + 0; q = y[i] + 0; if (p > q) exit 0; if (p < q) exit 1 }
    exit 0 }'
}

# first_version TEXT -> first N.N[.N...] found in the text
first_version() { printf '%s\n' "$1" | sed -n 's/.*[^0-9.]\([0-9][0-9]*\(\.[0-9][0-9]*\)\{1,\}\).*/\1/p; t; s/^\([0-9][0-9]*\(\.[0-9][0-9]*\)\{1,\}\).*/\1/p' | head -n 1; }

# sha256_of FILE
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
  else echo "no sha256sum or shasum found" >&2; return 1; fi
}

# checksum_ok FILE EXPECTED -> success only when the file exists and its SHA-256 equals EXPECTED
checksum_ok() {
  [ -f "$1" ] || return 1
  case "$2" in *[!0-9a-fA-F]*|"") return 1 ;; esac
  [ "${#2}" -eq 64 ] || return 1
  [ "$(sha256_of "$1")" = "$(printf '%s' "$2" | tr 'A-F' 'a-f')" ]
}

# sha_from_text TEXT -> first 64-hex token
sha_from_text() { printf '%s\n' "$1" | tr 'A-F' 'a-f' | grep -Eo '(^|[^0-9a-f])[0-9a-f]{64}([^0-9a-f]|$)' | head -n 1 | grep -Eo '[0-9a-f]{64}'; }

# zig_index_sha INDEX_JSON_TEXT URL -> the shasum that follows the tarball URL in ziglang.org's index.json
zig_index_sha() { printf '%s\n' "$1" | awk -v url="$2" 'index($0, "\"" url "\"") { f = 1 } f && /"shasum"/ { s = $0; sub(/.*"shasum"[ \t]*:[ \t]*"/, "", s); sub(/".*/, "", s); print s; exit }'; }

# os_family OS_RELEASE_FILE -> debian | fedora | arch | suse | (empty). Uses ID and ID_LIKE against packages.toml.
os_family() {
  local id="" like="" line fam word ids
  [ -r "$1" ] || return 0
  while IFS= read -r line; do
    case "$line" in
      ID=*) id=$(printf '%s' "${line#ID=}" | tr -d '"' | tr 'A-Z' 'a-z') ;;
      ID_LIKE=*) like=$(printf '%s' "${line#ID_LIKE=}" | tr -d '"' | tr 'A-Z' 'a-z') ;;
    esac
  done < "$1"
  # The distribution itself first, then its relatives.
  for word in $id $like; do
    for fam in debian fedora arch suse; do
      ids=$(cfg "linux.$fam.ids")
      case " $ids " in *" $word "*) printf '%s\n' "$fam"; return 0 ;; esac
    done
  done
}

# resolve_components "a,b all ..." -> canonical ordered list with dependencies, one line, space separated
resolve_components() {
  local want="" part c out="" needs n
  for part in $(printf '%s' "$1" | tr ',' ' '); do
    part=$(printf '%s' "$part" | tr 'A-Z' 'a-z')
    if [ "$part" = all ]; then want="$ALL_COMPONENTS"; continue; fi
    case " $ALL_COMPONENTS " in *" $part "*) want="$want $part" ;; *) echo "unknown component '$part'; known: ${ALL_COMPONENTS// /, }, all" >&2; return 1 ;; esac
  done
  for c in $ALL_COMPONENTS; do
    case " $want " in *" $c "*) ;; *) continue ;; esac
    needs=$(cfg "component.$c.needs")
    for n in $needs; do case " $out " in *" $n "*) ;; *) out="$out $n" ;; esac; done
    case " $out " in *" $c "*) ;; *) out="$out $c" ;; esac
  done
  # keep the canonical order
  local final=""
  for c in $ALL_COMPONENTS; do case " $out " in *" $c "*) final="$final $c" ;; esac; done
  printf '%s\n' "${final# }"
}

# esc_pref VALUE -> editor.ron escaping ('%' and ',' ; crates/engine-editor/src/prefs.rs)
esc_pref() { printf '%s' "$1" | sed -e 's/%/%25/g' -e 's/,/%2C/g'; }

# merge_prefs TEXT_FILE VALUES_FILE(key<TAB>value) [OVERWRITE_KEYS...] -> new text on stdout; "SET key" / "KEPT key" on stderr.
# Adds missing or empty keys; keeps non-empty user values unless the key is listed for overwrite.
merge_prefs() {
  local text="$1" values="$2"; shift 2
  local ow="$*"
  awk -F'\t' -v ow=" $ow " '
    FILENAME == ARGV[1] { vk[++n] = $1; vv[n] = $2; next }
    { lines[++m] = $0 }
    END {
      if (m == 0) { lines[++m] = "("; lines[++m] = ")" }
      while (m > 0 && lines[m] ~ /^[ \t\r]*$/) m--
      close_i = 0
      for (i = m; i >= 1; i--) { t = lines[i]; gsub(/[ \t\r]/, "", t); if (t == ")") { close_i = i; break } }
      if (close_i == 0) { print "no closing paren in editor.ron" > "/dev/stderr"; exit 3 }
      for (j = 1; j <= n; j++) {
        found = 0
        for (i = 1; i < close_i; i++) {
          if (match(lines[i], "^[ \t]*" vk[j] "[ \t]*:")) {
            found = i
            cur = substr(lines[i], RSTART + RLENGTH); gsub(/^[ \t]+|[ \t,\r]+$/, "", cur)
            if (cur == vv[j]) { status[j] = "same" }
            else if (cur == "" || index(ow, " " vk[j] " ") > 0) { lines[i] = "    " vk[j] ": " vv[j] ","; status[j] = "set" }
            else { status[j] = "kept" }
            break
          }
        }
        if (!found) { add[++na] = "    " vk[j] ": " vv[j] ","; status[j] = "set" }
      }
      for (i = 1; i < close_i; i++) print lines[i]
      for (i = 1; i <= na; i++) print add[i]
      for (i = close_i; i <= m; i++) print lines[i]
      for (j = 1; j <= n; j++) if (status[j] == "set") print "SET " vk[j] > "/dev/stderr"; else if (status[j] == "kept") print "KEPT " vk[j] > "/dev/stderr"
    }' "$values" "$text"
}

# add_block TEXT ID BLOCK -> TEXT plus a delimited block (unchanged when the block already exists)
add_block() {
  local begin="# >>> piska-engine installer: $2 >>>" end="# <<< piska-engine installer: $2 <<<"
  case "$1" in *"$begin"*) printf '%s' "$1"; return 0 ;; esac
  printf '%s' "$1"
  [ -n "$1" ] && case "$1" in *$'\n') ;; *) printf '\n' ;; esac
  printf '\n%s\n%s\n%s\n' "$begin" "$3" "$end"
}

# remove_block FILE ID -> text of the file without the delimited block
remove_block() { awk -v b="# >>> piska-engine installer: $2 >>>" -v e="# <<< piska-engine installer: $2 <<<" '$0 == b { skip = 1; next } $0 == e { skip = 0; next } !skip { print }' "$1"; }

# uninstall_plan MANIFEST -> "action<TAB>target<TAB>ask" lines, newest first. Only what the installer recorded as added.
uninstall_plan() {
  [ -f "$1" ] || return 0
  awk -F'\t' '{ k[NR] = $1; id[NR] = $2; d[NR] = $3 } END {
    for (i = NR; i >= 1; i--) {
      if (k[i] == "file" || k[i] == "shortcut") print "delete-file\t" d[i] "\t0"
      else if (k[i] == "block") print "remove-block\t" d[i] " (" id[i] ")\t0"
      else if (k[i] == "prefs") print "restore-prefs\t" d[i] "\t1"
      else if (k[i] == "cargo-crate") print "cargo-uninstall\t" id[i] "\t0"
      else if (k[i] == "rust-target") print "rustup-target-remove\t" id[i] "\t0"
      else if (k[i] == "dir") print "delete-dir\t" d[i] "\t1"
      else if (k[i] == "rustup") print "rustup-self-uninstall\trustup\t1"
      else if (k[i] == "pkg") print "note-packages\t" d[i] " (left installed: remove with your package manager if you want)\t0"
    }}' "$1"
}

# step_skip MODE STATE_FILE ID DETECTED(0|1) -> success when the step is skipped
step_skip() {
  [ "$4" = 1 ] && return 0
  if [ "$1" = resume ] && [ -f "$2" ]; then awk -F'\t' -v id="$3" '$1 == id { s = $2 } END { exit (s == "done" ? 0 : 1) }' "$2" && return 0; fi
  return 1
}

# ---------------------------------------------------------------------------------------
# Machine-readable output (--json): one JSON object per line on the original stdout (fd 3)
# ---------------------------------------------------------------------------------------

# json_str TEXT -> "TEXT" with JSON escapes
json_str() {
  local s=$1
  s=${s//\\/\\\\}; s=${s//\"/\\\"}; s=${s//$'\t'/\\t}; s=${s//$'\r'/\\r}; s=${s//$'\n'/\\n}
  printf '"%s"' "$s"
}
jn() { if [ -n "$1" ]; then json_str "$1"; else printf null; fi; }        # string or null
jkv() { printf ',"%s":%s' "$1" "$(json_str "$2")"; }                    # ,"key":"string"
jkn() { printf ',"%s":%s' "$1" "$(jn "$2")"; }                          # ,"key":"string"|null
jkr() { printf ',"%s":%s' "$1" "${2:-null}"; }                          # ,"key":raw (number, bool, array, object)
jbool() { if "$@"; then printf true; else printf false; fi; }
# event NAME FIELDS -> {"event":"NAME"FIELDS}; written to fd 3 so that it also leaves command substitutions
event() { printf '{"event":"%s"%s}\n' "$1" "${2:-}" >&3; }
# json_yes LINE -> success for an explicit yes
json_yes() { case "$(printf '%s' "$1" | tr 'A-Z' 'a-z' | tr -d ' \r')" in y|yes|true|accept|*'"answer":true'*|*'"answer":"yes"'*) return 0 ;; *) return 1 ;; esac; }
# json_prompt KIND FIELDS -> asks with a "prompt" event, reads one line from the original stdin (fd 4); end of input = no
json_prompt() {
  local ans
  event prompt "$(jkv kind "$1")$(jkn step "$CURRENT_STEP")$2"
  IFS= read -r ans <&4 || return 1
  json_yes "$ans"
}
# plan_lines_json < "kind     text" lines -> [{"kind":..,"text":..}, ...]
plan_lines_json() {
  local out="" l kind text
  while IFS= read -r l; do
    [ -n "$l" ] || continue
    kind=${l%% *}; text=${l#"$kind"}; text=${text#"${text%%[! ]*}"}
    out="$out${out:+,}{\"kind\":$(json_str "$kind"),\"text\":$(json_str "$text")}"
  done
  printf '[%s]' "$out"
}
# download_json SECTION -> the download entry of packages.toml as a JSON object (empty when not defined)
download_json() {
  local s=$1 url pin surl size idx ps cross=null
  url=$(cfg "$s.url"); [ -n "$url" ] || return 0
  pin=$(cfg "$s.sha256"); surl=$(cfg "$s.sha256_url"); size=$(cfg "$s.size_mb"); idx=$(cfg "$s.index_key"); ps=$(cfg "$s.pin_status")
  [ -n "$idx" ] && cross=$(json_str "$(cfg zig.index_url)")
  printf '{"name":%s,"url":%s,"size_mb":%s,"sha256":%s,"sha256_url":%s,"pinned":%s,"pin_status":%s,"cross_check":%s}' \
    "$(json_str "${s#download.}")" "$(json_str "$url")" "${size:-null}" "$(jn "$pin")" "$(jn "$surl")" "$(jbool [ -n "$pin" ])" "$(jn "$ps")" "$cross"
}
step_download_sections() { # step_download_sections ID -> packages.toml sections the step downloads
  case "$1" in
    core.rustup) dl_key rustup ;;
    android.cmdline-tools) { [ "$OS" = linux ] && [ "$ARCH" != x86_64 ]; } || dl_key android ;;
    linux-cross.zig) dl_key zig ;;
  esac
}
in_list() { case " $2 " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }   # in_list WORD "LIST"
# skip_requested ID -> success when --skip-steps names this step and it may be skipped
skip_requested() {
  in_list "$1" "$(printf '%s' "${SKIP_STEPS:-}" | tr ',' ' ')" || return 1
  in_list "$1" "$SKIPPABLE_STEPS" && return 0
  warn "step $1 cannot be skipped (later steps need it): it runs as usual"; return 1
}
# found_version TEXT / found_path TEXT -> details of a detection text for the "plan" event
found_version() { case "$1" in /*|"all "*|"not needed"*|checkout*|clone*) return 0 ;; esac; local h=${1%% at *}; first_version "$h" | head -n 1; case "$h" in "JDK "[0-9]*) [ -z "$(first_version "$h")" ] && printf '%s\n' "${h#JDK }" ;; esac; }
found_path() { case "$1" in /*) printf '%s\n' "$1" ;; *" at /"*) printf '%s\n' "/${1#* at /}" ;; "checkout /"*) printf '%s\n' "${1#checkout }" ;; esac; }
# free_bytes DIR -> free space (bytes) where DIR is or would be created; empty when unknown
free_bytes() {
  local p=$1
  while [ -n "$p" ] && [ ! -d "$p" ]; do p=$(dirname "$p"); [ "$p" = "/" ] && break; done
  df -Pk "$p" 2>/dev/null | awk 'NR == 2 { printf "%.0f\n", $4 * 1024 }'
}
write_hello() {
  event hello "$(jkr protocol 1)$(jkv script install.sh)$(jkv shell "bash $BASH_VERSION")$(jkv os "$OS")$(jkn family "$FAMILY")$(jkv arch "$ARCH")$(jkv mode "$MODE")$(jkr dry_run "$(jbool [ "$DRY_RUN" = 1 ])")$(jkr assume_clean "$(jbool [ "$ASSUME_CLEAN" = 1 ])")$(jkr components "$(printf '%s\n' $COMPONENTS | jarr)")$(jkv install_dir "$PREFIX")$(jkv repo_dir "$REPO_DIR")$(jkr in_checkout "$(jbool [ "$IN_CHECKOUT" = 1 ])")$(jkv cache "$CACHE")$(jkv state_file "$STATE_FILE")$(jkv manifest_file "$MANIFEST_FILE")$(jkv prefs_file "$(prefs_path)")$(jkn log "$LOG_FILE")$(jkr free_bytes "$(free_bytes "$PREFIX")")$(jkr is_admin "$(jbool [ "$(id -u)" = 0 ])")$(jkv elevate "$ELEVATE")"
}
jarr() { local out="" l; while IFS= read -r l; do [ -n "$l" ] && out="$out${out:+,}$(json_str "$l")"; done; printf '[%s]' "$out"; }   # lines -> ["..", ..]
write_summary() { # write_summary EXIT_CODE
  local envs="" n v
  for n in ANDROID_HOME ANDROID_NDK_HOME JAVA_HOME PISKA_ZIG; do v=$(eval "printf '%s' \"\${$n:-}\""); [ -n "$v" ] && envs="$envs${envs:+,}$(json_str "$n"):$(json_str "$v")"; done
  event summary "$(jkr exit "$1")$(jkr ok "$(jbool [ "$1" = 0 ])")$(jkr dry_run "$(jbool [ "$DRY_RUN" = 1 ])")$(jkn failed_step "$FAILED_STEP")$(jkn error "$FAILED_ERROR")$(jkn manual_command "$MANUAL_COMMAND")$(jkr reboot "$(jbool [ "$REBOOT_NEEDED" = 1 ])")$(jkr elevation_refused false)$(jkr steps "{\"total\":$STAT_TOTAL,\"done\":$STAT_DONE,\"skipped\":$STAT_SKIPPED,\"failed\":$STAT_FAILED}")$(jkn log "${LOG_FILE:-}")$(jkn state_file "${STATE_FILE:-}")$(jkn manifest_file "${MANIFEST_FILE:-}")$(jkn repo_dir "${REPO_DIR:-}")$(jkv path "$(cargo_bin_dir):$PATH")$(jkr env "{$envs}")"
}

# ---------------------------------------------------------------------------------------
# Output, state, manifest
# ---------------------------------------------------------------------------------------

say() { if [ "$JSON" = 1 ]; then event log "$(jkv level info)$(jkn step "$CURRENT_STEP")$(jkv text "$*")"; else printf '%s\n' "$*"; fi; log_line "$*"; }
warn() { if [ "$JSON" = 1 ]; then event log "$(jkv level warn)$(jkn step "$CURRENT_STEP")$(jkv text "$*")"; else printf 'warning: %s\n' "$*" >&2; fi; log_line "warning: $*"; }
die() { printf 'error: %s\n' "$*" >&2; exit "$EXIT_FAILED"; }
log_line() { if [ -n "${LOG_FILE:-}" ] && [ "$DRY_RUN" != 1 ]; then printf '%s %s\n' "$(date +%Y-%m-%dT%H:%M:%S)" "$*" >> "$LOG_FILE" 2>/dev/null || true; fi; }

confirm() { # confirm QUESTION DEFAULT(y|n) [KEY SUBJECT]  (KEY / SUBJECT identify the question for --json)
  if [ "$YES" = 1 ]; then return 0; fi
  if [ "$JSON" = 1 ]; then json_prompt confirm "$(jkv key "${3:-confirm}")$(jkv subject "${4:-}")$(jkv question "$1")$(jkr default "$(jbool [ "${2:-y}" = y ])")"; return; fi
  [ -t 0 ] || return 1
  local ans suffix="[Y/n]"; [ "${2:-y}" = n ] && suffix="[y/N]"
  printf '%s %s ' "$1" "$suffix"; read -r ans || return 1
  [ -z "$ans" ] && { [ "${2:-y}" = y ]; return; }
  case "$ans" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

confirm_license() { # confirm_license NAME URL [ID]
  [ "$ACCEPT_LICENSES" = 1 ] && return 0
  [ -n "${3:-}" ] && in_list "$3" "$(printf '%s' "${ACCEPT_LICENSE:-}" | tr ',' ' ')" && return 0
  if [ "$YES" = 1 ]; then echo "error: '$1' needs you to accept its licence ($2). Re-run with --accept-licenses, or without --yes to be asked." >&2; return 1; fi
  if [ "$JSON" = 1 ]; then json_prompt licence "$(jkn licence "${3:-}")$(jkv name "$1")$(jkv url "$2")"; return; fi
  confirm "Do you accept the licence of $1 ($2)?" n
}

manifest_add() { # kind id detail previous component
  [ "$DRY_RUN" = 1 ] && return 0
  mkdir -p "$STATE_DIR"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "${3:-}" "${4:-}" "${5:-}" "$(date +%Y-%m-%dT%H:%M:%S)" >> "$MANIFEST_FILE"
}

state_set() { # id status [error]
  [ "$DRY_RUN" = 1 ] && return 0
  mkdir -p "$STATE_DIR"
  { [ -f "$STATE_FILE" ] && awk -F'\t' -v id="$1" '$1 != id' "$STATE_FILE"; printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$(date +%Y-%m-%dT%H:%M:%S)" "${3:-}"; } > "$STATE_FILE.new" && mv "$STATE_FILE.new" "$STATE_FILE"
}

backup_file() { [ -f "$1" ] || return 0; local b; b="$1.piska-backup-$(date +%Y%m%d-%H%M%S)"; cp "$1" "$b" && printf '%s' "$b"; }

# have CMD -> success when the command exists (always false with --assume-clean)
have() { [ "$ASSUME_CLEAN" = 1 ] && return 1; command -v "$1" >/dev/null 2>&1; }
fact_dir() { [ "$ASSUME_CLEAN" = 1 ] && return 1; [ -d "$1" ]; }
fact_file() { [ "$ASSUME_CLEAN" = 1 ] && return 1; [ -f "$1" ]; }

tool_version() { # tool_version CMD ARGS... -> version or empty
  local out; out=$("$@" 2>&1) || return 0
  first_version "$out"
}

# ---------------------------------------------------------------------------------------
# Platform
# ---------------------------------------------------------------------------------------

detect_platform() {
  case "${ASSUME_OS:-}" in
    macos) OS=macos; FAMILY=macos ;;
    debian|fedora|arch|suse) OS=linux; FAMILY=$ASSUME_OS ;;
    "")
      case "$(uname -s)" in
        Darwin) OS=macos; FAMILY=macos ;;
        Linux) OS=linux; FAMILY=$(os_family "${PISKA_OS_RELEASE:-/etc/os-release}") ;;
        *) echo "This installer supports Linux and macOS. On Windows use install\\install.ps1." >&2
           [ "$JSON" = 1 ] && { event error "$(jkv text "this installer supports Linux and macOS; on Windows use install.ps1")"; write_summary "$EXIT_BAD_ARGS"; }
           exit "$EXIT_BAD_ARGS" ;;
      esac ;;
    *) echo "--assume-os: unknown value '$ASSUME_OS'" >&2; exit "$EXIT_BAD_ARGS" ;;
  esac
  ARCH=${ASSUME_ARCH:-$(uname -m)}
  case "$ARCH" in arm64) ARCH=aarch64 ;; amd64) ARCH=x86_64 ;; esac
  if [ -z "$FAMILY" ]; then
    echo "error: this Linux distribution is not recognised (supported: Debian/Ubuntu, Fedora, Arch, openSUSE)." >&2
    echo "Install the packages listed in docs/install.md by hand, then run: install.sh --components <list> (the package step is skipped with --skip-packages)." >&2
    SKIP_PACKAGES=1
  fi
}

host_triple() {
  case "$OS-$ARCH" in
    linux-x86_64) echo x86_64-unknown-linux-gnu ;; linux-aarch64) echo aarch64-unknown-linux-gnu ;;
    macos-x86_64) echo x86_64-apple-darwin ;; macos-aarch64) echo aarch64-apple-darwin ;;
  esac
}

# ---------------------------------------------------------------------------------------
# System packages
# ---------------------------------------------------------------------------------------

pkg_installed() {
  [ "$ASSUME_CLEAN" = 1 ] && return 1
  case "$FAMILY" in
    debian) dpkg -s "$1" >/dev/null 2>&1 ;;
    fedora|suse) rpm -q "$1" >/dev/null 2>&1 ;;
    arch) pacman -Q "$1" >/dev/null 2>&1 ;;
    macos) brew list --formula "$1" >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}

pkgs_for() { # pkgs_for KINDS... -> union of the package lists for the family
  local kind out=""
  for kind in "$@"; do out="$out $(cfg "$(pkg_section).$kind")"; done
  printf '%s\n' $out | awk 'NF && !seen[$0]++' | tr '\n' ' ' | sed 's/ $//'
}

pkg_section() { if [ "$FAMILY" = macos ]; then echo macos; else echo "linux.$FAMILY"; fi; }

missing_pkgs() { local p out=""; for p in $1; do pkg_installed "$p" || out="$out $p"; done; printf '%s\n' "${out# }"; }

as_root() { if [ "$(id -u)" = 0 ]; then "$@"; elif command -v sudo >/dev/null 2>&1; then sudo "$@"; else echo "error: administrator rights are needed (no sudo found): run this script as root or install sudo" >&2; return 1; fi; }
root_prefix() { if [ "$(id -u)" = 0 ] || [ "$FAMILY" = macos ]; then :; else printf 'sudo '; fi; }

pkg_install_lines() { # pkg_install_lines "pkgs" -> display lines
  local sec update install; sec=$(pkg_section); update=$(cfg "$sec.update"); install=$(cfg "$sec.install")
  [ "$update" != true ] && printf 'run      %s%s\n' "$(root_prefix)" "$update"
  printf 'run      %s%s %s\n' "$(root_prefix)" "$install" "$1"
}

pkg_install_run() { # pkg_install_run "pkgs"
  local sec update install; sec=$(pkg_section); update=$(cfg "$sec.update"); install=$(cfg "$sec.install")
  if [ "$FAMILY" = macos ]; then
    command -v brew >/dev/null 2>&1 || { echo "error: Homebrew is not installed. Install it from https://brew.sh (this installer does not run remote scripts for you), then re-run." >&2; return 1; }
    # shellcheck disable=SC2086
    $install $1 || return 1
  elif [ "$JSON" = 1 ] && [ "$(id -u)" != 0 ] && [ "$ELEVATE" != sudo ]; then
    # The graphical installer never handles a password: pkexec asks in its own window, or the user runs the command.
    local both="$install $1"; [ "$update" != true ] && both="$update && $both"
    if [ "$ELEVATE" = pkexec ] && command -v pkexec >/dev/null 2>&1; then
      event elevation "$(jkn step "$CURRENT_STEP")$(jkv reason "the package manager needs administrator rights: pkexec asks for your password in its own window")$(jkv command "$both")$(jkr confirmed true)"
      run_cmd pkexec /bin/sh -c "$both" || return 1
    else
      if [ "$update" != true ]; then MANUAL_COMMAND="sudo $update && sudo $install $1"; else MANUAL_COMMAND="sudo $install $1"; fi
      echo "error: administrator rights are needed: run this command in a terminal, then retry: $MANUAL_COMMAND" >&2
      return 1
    fi
  else
    say "  The package manager needs administrator rights: sudo will ask for your password."
    # shellcheck disable=SC2086
    if [ "$update" != true ]; then as_root $update || return 1; fi
    # shellcheck disable=SC2086
    as_root $install $1 || return 1
  fi
  manifest_add pkg packages "$1" "" "$STEP_COMPONENT"
}

# ---------------------------------------------------------------------------------------
# Verified downloads
# ---------------------------------------------------------------------------------------

dl_key() { # dl_key rustup|zig|android -> section of packages.toml for this host
  case "$1" in
    rustup) echo "download.rustup_${OS}_${ARCH}" ;;
    zig) echo "download.zig_${OS}_${ARCH}" ;;
    android) if [ "$OS" = linux ]; then echo "download.android_cmdline_linux"; else echo "download.android_cmdline_macos_${ARCH}"; fi ;;
  esac
}

dl_describe() { # dl_describe SECTION -> display lines
  local url pin surl; url=$(cfg "$1.url"); pin=$(cfg "$1.sha256"); surl=$(cfg "$1.sha256_url")
  [ -n "$url" ] || { echo "note     no download is defined for this platform ($1)"; return; }
  printf 'download %s (~%s MB)\n' "$url" "$(cfg "$1.size_mb")"
  if [ -n "$pin" ]; then
    if [ -n "$(cfg "$1.index_key")" ]; then printf 'verify   sha256 %s (cross-checked with %s)\n' "$pin" "$(cfg zig.index_url)"; else printf 'verify   sha256 %s\n' "$pin"; fi
  else printf 'verify   sha256 from %s\n' "$surl"; fi
  printf 'cache    %s\n' "$CACHE"
}

assert_download() { # refuse entries without a checksum source
  local pin surl url; pin=$(cfg "$1.sha256"); surl=$(cfg "$1.sha256_url"); url=$(cfg "$1.url")
  [ -n "$url" ] || { echo "error: no download for this platform ($1)" >&2; return 1; }
  case "$url" in https://*) ;; *) echo "error: $1: not an https:// URL" >&2; return 1 ;; esac
  if [ -n "$pin" ]; then [ "${#pin}" -eq 64 ] || { echo "error: $1: malformed sha256" >&2; return 1; }
  else case "$surl" in https://*) ;; *) echo "error: $1 has neither sha256 nor sha256_url in packages.toml; refusing to use it" >&2; return 1 ;; esac; fi
}

curl_get() { # curl_get URL OUT   (TLS 1.2+, honours HTTPS_PROXY, fails on HTTP errors)
  curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 --output "$2" "$1"
}

# curl_get_progress URL OUT SIZE_MB -> curl_get with a "progress" event every second (total estimated from packages.toml)
curl_get_progress() {
  local pid size total=""
  [ -n "${3:-}" ] && total=$(( $3 * 1048576 ))
  curl_get "$1" "$2" & pid=$!
  while kill -0 "$pid" 2>/dev/null; do
    size=$(wc -c < "$2" 2>/dev/null | tr -d ' ')
    event progress "$(jkn step "$CURRENT_STEP")$(jkv url "$1")$(jkr bytes "${size:-0}")$(jkr total "${total:-null}")$(jkr estimated true)"
    sleep 1
  done
  wait "$pid"
}

curl_text() { curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 "$1"; }

# verified_file SECTION -> prints the path of a downloaded (or cached) file that passed its checksum
verified_file() {
  local sec="$1" url pin expected file part
  assert_download "$sec" || return 1
  url=$(cfg "$sec.url"); pin=$(cfg "$sec.sha256")
  file="$CACHE/$(basename "$url")"
  expected=$pin
  if [ -z "$expected" ]; then
    expected=$(sha_from_text "$(curl_text "$(cfg "$sec.sha256_url")")")
    [ -n "$expected" ] || { echo "error: could not read a SHA-256 from $(cfg "$sec.sha256_url")" >&2; return 1; }
  fi
  if checksum_ok "$file" "$expected"; then echo "  using cached $file (checksum ok)" >&2; printf '%s\n' "$file"; return 0; fi
  if [ -n "$(cfg "$sec.index_key")" ]; then
    local from_index; from_index=$(zig_index_sha "$(curl_text "$(cfg zig.index_url)")" "$url")
    [ "$from_index" = "$expected" ] || { echo "error: zig checksum pinned in packages.toml ($expected) differs from index.json (${from_index:-none}): refusing to download" >&2; return 1; }
  fi
  mkdir -p "$CACHE"; part="$file.part"
  echo "  downloading $url" >&2
  if [ "$JSON" = 1 ]; then curl_get_progress "$url" "$part" "$(cfg "$sec.size_mb")" || { rm -f "$part"; echo "error: download failed: $url" >&2; return 1; }
  else curl_get "$url" "$part" || { rm -f "$part"; echo "error: download failed: $url" >&2; return 1; }; fi
  if ! checksum_ok "$part" "$expected"; then
    local actual; actual=$(sha256_of "$part"); rm -f "$part"
    echo "error: checksum mismatch for $url: expected $expected, got $actual. The file was deleted and NOT used." >&2; return 1
  fi
  mv "$part" "$file"; echo "  sha256 ok: $expected" >&2; printf '%s\n' "$file"
}

# ---------------------------------------------------------------------------------------
# Locations
# ---------------------------------------------------------------------------------------

sdk_root() {
  if [ -n "$ANDROID_SDK_ARG" ]; then echo "$ANDROID_SDK_ARG"; return; fi
  local v
  if [ "$ASSUME_CLEAN" != 1 ]; then
    for v in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}"; do [ -n "$v" ] && [ -d "$v" ] && { echo "$v"; return; }; done
  fi
  for v in "$HOME/Android/Sdk" "$HOME/Library/Android/sdk"; do
    if fact_dir "$v/platforms" || fact_dir "$v/cmdline-tools" || fact_dir "$v/platform-tools"; then echo "$v"; return; fi
  done
  if [ "$OS" = macos ]; then echo "$HOME/Library/Android/sdk"; else echo "$HOME/Android/Sdk"; fi
}

zig_dir() { echo "$PREFIX/tools/zig/$(cfg zig.version)"; }
zig_bin() { echo "$(zig_dir)/zig-$OS-$ARCH-$(cfg zig.version)/zig"; }
# exe_path NAME -> where a release build puts it (CARGO_TARGET_DIR, else target/; the modeler is its own workspace in modeler/)
exe_path() {
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then echo "$CARGO_TARGET_DIR/release/$1"
  elif [ "$1" = "$(cfg project.modeler_binary)" ]; then echo "$REPO_DIR/modeler/target/release/$1"
  else echo "$REPO_DIR/target/release/$1"; fi
}
cargo_bin_dir() { echo "${CARGO_HOME:-$HOME/.cargo}/bin"; }
# NOTE: the editor reads $HOME/.config/PiskaEngine/editor.ron (crates/engine-editor/src/prefs.rs); XDG_CONFIG_HOME is not consulted there.
prefs_path() { echo "$HOME/.config/PiskaEngine/editor.ron"; }
desktop_dir() { local d=""; command -v xdg-user-dir >/dev/null 2>&1 && d=$(xdg-user-dir DESKTOP 2>/dev/null); echo "${d:-$HOME/Desktop}"; }
env_file() { echo "$PREFIX/env.sh"; }

profile_files() {
  local f out=""
  if [ "$OS" = macos ]; then out="$HOME/.zprofile"
  else
    if [ -f "$HOME/.bash_profile" ]; then out="$HOME/.bash_profile"; else out="$HOME/.profile"; fi
    [ -f "$HOME/.zprofile" ] && out="$out $HOME/.zprofile"
  fi
  printf '%s\n' "$out"
}

find_rustc() { have rustc && command -v rustc && return 0; fact_file "$(cargo_bin_dir)/rustc" && echo "$(cargo_bin_dir)/rustc"; }

jdk_major() { # jdk_major -> major version of java found, or empty
  local out v
  have java || return 0
  out=$(java -version 2>&1 | head -n 1)
  v=$(printf '%s' "$out" | sed -n 's/.*version "\([0-9]*\)\(\.\([0-9]*\)\)\{0,1\}.*/\1 \3/p')
  set -- $v
  if [ "${1:-}" = 1 ]; then echo "${2:-}"; else echo "${1:-}"; fi
}

jdk_home() {
  if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ] && [ "$ASSUME_CLEAN" != 1 ]; then echo "$JAVA_HOME"; return; fi
  have java || return 0
  local j; j=$(command -v java)
  j=$(readlink -f "$j" 2>/dev/null || echo "$j")
  dirname "$(dirname "$j")"
}

rust_target_installed() { # rust_target_installed TARGET
  local rustc root; rustc=$(find_rustc) || return 1
  [ -n "$rustc" ] || return 1
  root=$("$rustc" --print sysroot 2>/dev/null) || return 1
  [ -d "$root/lib/rustlib/$1" ]
}

sdk_package_present() { # sdk_package_present SDK PACKAGE
  local sdk="$1" kind d min_api min_ndk
  kind=${2%%;*}; min_api=$(cfg min.android_api); min_ndk=$(cfg min.ndk_major)
  [ "$ASSUME_CLEAN" = 1 ] && return 1
  case "$kind" in
    platform-tools) [ -d "$sdk/platform-tools" ] ;;
    platforms) for d in "$sdk"/platforms/android-*; do [ -f "$d/android.jar" ] || continue; n=${d##*/android-}; n=${n%%.*}; [ "$n" -ge "$min_api" ] 2>/dev/null && return 0; done; return 1 ;;
    build-tools) for d in "$sdk"/build-tools/*; do [ -f "$d/aapt2" ] && [ -f "$d/zipalign" ] && return 0; done; return 1 ;;
    ndk) for d in "$sdk"/ndk/*; do [ -f "$d/source.properties" ] || continue; n=${d##*/}; n=${n%%.*}; [ "$n" -ge "$min_ndk" ] 2>/dev/null && return 0; done; return 1 ;;
    *) return 1 ;;
  esac
}

missing_sdk_packages() { local sdk p out=""; sdk=$(sdk_root); for p in $(cfg android.sdk_packages | tr ';' '|' | tr ' ' '\n' | tr '|' ';'); do sdk_package_present "$sdk" "$p" || out="$out $p"; done; printf '%s\n' "${out# }"; }

# ---------------------------------------------------------------------------------------
# Steps: step_title / step_detect / step_plan / step_run, selected by id.
# detect prints what it found and succeeds when the step can be skipped.
# ---------------------------------------------------------------------------------------

build_steps() { # build_steps "components" -> step ids, one per line
  local c
  for c in $1; do
    case "$c" in
      core) [ "${SKIP_PACKAGES:-0}" = 1 ] || echo core.packages; echo core.rustup; echo core.rust-components; echo core.vulkan ;;
      cpp) [ "${SKIP_PACKAGES:-0}" = 1 ] || echo cpp.packages ;;
      web) echo web.target ;;
      android) [ "${SKIP_PACKAGES:-0}" = 1 ] || echo android.jdk; echo android.cmdline-tools; echo android.licenses; echo android.packages; echo android.cargo-ndk; echo android.target; echo android.env ;;
      linux-cross) echo linux-cross.zig; echo linux-cross.target; echo linux-cross.env ;;
      engine) echo engine.source; echo engine.build; [ "$OS" = linux ] && [ "${NO_SHORTCUTS:-0}" != 1 ] && echo engine.desktop; [ "$FILE_ASSOC" = 1 ] && [ "$OS" = linux ] && echo engine.assoc; echo engine.uninstaller ;;
    esac
  done
  [ "$NO_PREFS" = 1 ] || echo config.preferences
  [ -n "$CARGO_TARGET_DIR_ARG" ] && echo config.cargo
  return 0
}

step_title() {
  case "$1" in
    core.packages) echo "System packages (compiler, pkg-config, ALSA, udev, Vulkan, X11/Wayland libraries)" ;;
    core.rustup) echo "Rust (rustup, stable)" ;;
    core.rust-components) echo "Rust components ($(cfg rust.components))" ;;
    core.vulkan) echo "Vulkan runtime (GPU driver)" ;;
    cpp.packages) echo "CMake + Ninja" ;;
    web.target) echo "Rust target $(cfg rust.web_target)" ;;
    android.jdk) echo "JDK $(cfg android.jdk)" ;;
    android.cmdline-tools) echo "Android command-line tools (sdkmanager)" ;;
    android.licenses) echo "Android SDK licences" ;;
    android.packages) echo "Android SDK packages" ;;
    android.cargo-ndk) echo "cargo-ndk $(cfg android.cargo_ndk)" ;;
    android.target) echo "Rust target $(cfg rust.android_targets)" ;;
    android.env) echo "Environment: ANDROID_HOME, ANDROID_NDK_HOME (and JAVA_HOME if unset)" ;;
    linux-cross.zig) echo "zig $(cfg zig.version)" ;;
    linux-cross.target) echo "Rust target $(cfg rust.linux_target)" ;;
    linux-cross.env) echo "Environment: PISKA_ZIG" ;;
    engine.source) echo "Engine sources (with the modeler submodule)" ;;
    engine.build) echo "Build hub, editor, export tool and modeler (release)" ;;
    engine.desktop) echo "Application-menu entries and icon" ;;
    engine.assoc) echo "File associations (.pmodel, .scene, .scenario, .$(cfg project.project_extension))" ;;
    engine.uninstaller) echo "Uninstall script" ;;
    config.preferences) echo "Editor preferences: Build & Tools paths" ;;
    config.cargo) echo "Cargo config: shared target dir" ;;
  esac
}

step_needs_root() { case "$1" in core.packages|cpp.packages|android.jdk) [ "$FAMILY" != macos ] ;; *) return 1 ;; esac; }

step_licence() { case "$1" in android.licenses) echo "the Android SDK|https://developer.android.com/studio/terms" ;; esac; }
step_licence_id() { case "$1" in android.licenses) echo android-sdk ;; esac; }

pkgs_of_step() {
  case "$1" in
    core.packages) pkgs_for base build run ;;
    cpp.packages) pkgs_for cpp ;;
    android.jdk) if [ "$FAMILY" = macos ]; then echo ""; else pkgs_for jdk; fi ;;
  esac
}

step_detect() { # prints found text; success = skip
  local id="$1" v rustc sdk pkgs
  case "$id" in
    core.packages|cpp.packages)
      pkgs=$(pkgs_of_step "$id"); local miss; miss=$(missing_pkgs "$pkgs")
      if [ "$FAMILY" = macos ] && [ "$id" = core.packages ] && ! have xcode-select; then return 1; fi
      [ -z "$miss" ] && { echo "all packages present"; return 0; }
      return 1 ;;
    core.rustup)
      rustc=$(find_rustc) || return 1
      v=$(tool_version "$rustc" --version); [ -n "$v" ] && version_ge "$v" "$(cfg min.rust)" && { echo "rustc $v"; return 0; }; return 1 ;;
    core.rust-components)
      have rustup || { fact_file "$(cargo_bin_dir)/rustup" || return 1; }
      local list; list=$("$(command -v rustup || echo "$(cargo_bin_dir)/rustup")" component list --installed 2>/dev/null) || return 1
      case "$list" in *clippy*rustfmt*|*rustfmt*clippy*) echo installed; return 0 ;; esac; return 1 ;;
    core.vulkan)
      [ "$OS" = macos ] && { echo "Metal (nothing to install)"; return 0; }
      for v in /usr/lib/x86_64-linux-gnu /usr/lib/aarch64-linux-gnu /usr/lib64 /usr/lib /lib/x86_64-linux-gnu /lib64; do fact_file "$v/libvulkan.so.1" && { echo "$v/libvulkan.so.1"; return 0; }; done; return 1 ;;
    web.target) rust_target_installed "$(cfg rust.web_target)" && { echo "$(cfg rust.web_target)"; return 0; }; return 1 ;;
    android.jdk)
      v=$(jdk_major); [ -n "$v" ] && [ "$v" -ge "$(cfg min.java)" ] 2>/dev/null && { echo "JDK $v at $(jdk_home)"; return 0; }; return 1 ;;
    android.cmdline-tools)
      sdk=$(sdk_root)
      fact_file "$sdk/cmdline-tools/latest/bin/sdkmanager" && { echo "$sdk/cmdline-tools/latest/bin/sdkmanager"; return 0; }
      [ "$ASSUME_CLEAN" != 1 ] && [ -z "$(missing_sdk_packages)" ] && { echo "not needed: every SDK package is already installed"; return 0; }; return 1 ;;
    android.licenses) fact_file "$(sdk_root)/licenses/android-sdk-license" && { echo accepted; return 0; }; return 1 ;;
    android.packages) [ "$ASSUME_CLEAN" != 1 ] && [ -z "$(missing_sdk_packages)" ] && { echo "all SDK packages"; return 0; }; return 1 ;;
    android.cargo-ndk)
      have cargo || fact_file "$(cargo_bin_dir)/cargo" || return 1
      v=$(tool_version "$(command -v cargo || echo "$(cargo_bin_dir)/cargo")" ndk --version); [ -n "$v" ] && version_ge "$v" "$(cfg android.cargo_ndk)" && { echo "cargo-ndk $v"; return 0; }; return 1 ;;
    android.target) rust_target_installed "$(cfg rust.android_targets)" && { echo "$(cfg rust.android_targets)"; return 0; }; return 1 ;;
    android.env) [ "$ASSUME_CLEAN" != 1 ] && [ -n "${ANDROID_HOME:-}" ] && [ -n "${ANDROID_NDK_HOME:-}" ] && { echo set; return 0; }; return 1 ;;
    linux-cross.zig)
      if [ -n "${PISKA_ZIG:-}" ] && [ "$ASSUME_CLEAN" != 1 ]; then v=$(tool_version "$PISKA_ZIG" version); else have zig && v=$(tool_version zig version) || v=""; fi
      [ -n "$v" ] && version_ge "$v" "$(cfg min.zig)" && { echo "zig $v"; return 0; }; return 1 ;;
    linux-cross.target)
      [ "$(cfg rust.linux_target)" = "$(host_triple)" ] && { echo "host target"; return 0; }
      rust_target_installed "$(cfg rust.linux_target)" && { echo "$(cfg rust.linux_target)"; return 0; }; return 1 ;;
    linux-cross.env) [ "$ASSUME_CLEAN" != 1 ] && [ -n "${PISKA_ZIG:-}" ] && [ -f "$PISKA_ZIG" ] && { echo set; return 0; }; return 1 ;;
    engine.source)
      if [ "$IN_CHECKOUT" = 1 ]; then fact_file "$REPO_DIR/$(cfg project.modeler_manifest)" && { echo "checkout $REPO_DIR"; return 0; }; return 1; fi
      if fact_dir "$REPO_DIR/.git"; then
        if [ -n "$(cd "$REPO_DIR" && git status --porcelain 2>/dev/null)" ]; then echo "clone at $REPO_DIR has local changes (left untouched)"; return 0; fi
      fi; return 1 ;;
    engine.build) local b ok=1; for b in $(cfg project.binaries) $(cfg project.modeler_binary); do fact_file "$(exe_path "$b")" || ok=0; done; [ $ok = 1 ] && { echo built; return 0; }; return 1 ;;
    engine.desktop) fact_file "$HOME/.local/share/applications/piska-hub.desktop" && { echo present; return 0; }; return 1 ;;
    engine.assoc) fact_file "$HOME/.local/share/mime/packages/piska-engine.xml" && { echo present; return 0; }; return 1 ;;
    engine.uninstaller) fact_file "$PREFIX/uninstall.sh" && { echo present; return 0; }; return 1 ;;
    config.preferences) return 1 ;;
    config.cargo) fact_file "${CARGO_HOME:-$HOME/.cargo}/config.toml" && grep -q 'piska-engine installer: target-dir' "${CARGO_HOME:-$HOME/.cargo}/config.toml" && { echo present; return 0; }; return 1 ;;
  esac
  return 1
}

prefs_values() { # prefs_values ACTUAL(0|1) -> key<TAB>value lines (no files are written)
  local v sdk ndk c
  sdk=$(sdk_root)
  if [ "$1" = 1 ]; then
    c=$(command -v cargo 2>/dev/null || true); [ -n "$c" ] || { [ -x "$(cargo_bin_dir)/cargo" ] && c="$(cargo_bin_dir)/cargo"; }
    [ -n "$c" ] && printf 'tool_cargo\t%s\n' "$(esc_pref "$c")"
    case " $COMPONENTS " in *" android "*)
      [ -d "$sdk" ] && printf 'tool_android_sdk\t%s\n' "$(esc_pref "$sdk")"
      ndk=$(ls -d "$sdk"/ndk/* 2>/dev/null | sort | tail -n 1); [ -n "$ndk" ] && printf 'tool_android_ndk\t%s\n' "$(esc_pref "$ndk")"
      v=$(jdk_home); [ -n "$v" ] && printf 'tool_java\t%s\n' "$(esc_pref "$v")" ;;
    esac
    case " $COMPONENTS " in *" linux-cross "*) [ -x "$(zig_bin)" ] && printf 'tool_zig\t%s\n' "$(esc_pref "$(zig_bin)")" ;; esac
    case " $COMPONENTS " in *" cpp "*) c=$(command -v cmake 2>/dev/null || true); [ -n "$c" ] && printf 'tool_cmake\t%s\n' "$(esc_pref "$c")"
      c=$(command -v c++ 2>/dev/null || command -v g++ 2>/dev/null || command -v clang++ 2>/dev/null || true); [ -n "$c" ] && printf 'tool_cxx\t%s\n' "$(esc_pref "$c")" ;; esac
  else
    printf 'tool_cargo\t%s\n' "$(cargo_bin_dir)/cargo"
    case " $COMPONENTS " in *" android "*) printf 'tool_android_sdk\t%s\ntool_android_ndk\t%s\ntool_java\t%s\n' "$sdk" "$sdk/ndk/$(cfg android.ndk)" "<JDK folder>" ;; esac
    case " $COMPONENTS " in *" linux-cross "*) printf 'tool_zig\t%s\n' "$(zig_bin)" ;; esac
    case " $COMPONENTS " in *" cpp "*) printf 'tool_cmake\t%s\ntool_cxx\t%s\n' "<cmake>" "<c++>" ;; esac
  fi
}

env_exports() { # lines for env.sh from the selected components
  local sdk; sdk=$(sdk_root)
  case " $COMPONENTS " in *" android "*)
    echo "export ANDROID_HOME=\"$sdk\""
    echo "export ANDROID_NDK_HOME=\"$sdk/ndk/$(cfg android.ndk)\""
    echo "[ -z \"\${JAVA_HOME:-}\" ] && export JAVA_HOME=\"<JDK folder>\"" ;;
  esac
  case " $COMPONENTS " in *" linux-cross "*) echo "export PISKA_ZIG=\"$(zig_bin)\"" ;; esac
}

step_plan() {
  local id="$1" sdk pkgs miss sec
  sdk=$(sdk_root)
  case "$id" in
    core.packages|cpp.packages|android.jdk)
      pkgs=$(pkgs_of_step "$id")
      if [ "$FAMILY" = macos ]; then
        if [ "$id" = android.jdk ]; then printf 'run      %s %s\n' "$(cfg macos.install_cask)" "$(cfg macos.jdk_cask)"; return; fi
        [ "$id" = core.packages ] && echo "note     Xcode Command Line Tools are required (xcode-select -p); missing: run 'xcode-select --install' and re-run"
        printf 'note     Homebrew must be installed (https://brew.sh); this installer does not install it\n'
      fi
      miss=$(missing_pkgs "$pkgs")
      [ -n "$miss" ] && pkg_install_lines "$miss"
      step_needs_root "$id" && echo "note     the package manager runs through sudo and asks for your password" ;;
    core.rustup)
      dl_describe "$(dl_key rustup)"
      echo "run      rustup-init -y --default-toolchain $(cfg rust.toolchain) --profile minimal" ;;
    core.rust-components) echo "run      rustup component add $(cfg rust.components)" ;;
    core.vulkan) echo "note     libvulkan.so.1 comes from the Vulkan loader + a driver (Mesa or NVIDIA); the package step installs the Mesa drivers." ;;
    web.target) echo "run      rustup target add $(cfg rust.web_target)" ;;
    android.cmdline-tools)
      if [ "$OS" = linux ] && [ "$ARCH" != x86_64 ]; then echo "note     the Android NDK has no Linux $ARCH host build: use an x86_64 machine for Android exports"; return; fi
      dl_describe "$(dl_key android)"
      echo "extract  to $sdk/cmdline-tools/latest" ;;
    android.licenses) echo "run      yes | sdkmanager --sdk_root=$sdk --licenses   (answers 'y' to every licence; only after you accept)" ;;
    android.packages) echo "run      sdkmanager --sdk_root=$sdk $(cfg android.sdk_packages | sed 's/\([^ ]*\)/"\1"/g')" ;;
    android.cargo-ndk) echo "run      cargo install cargo-ndk --version $(cfg android.cargo_ndk) --locked   (compiles from crates.io, a few minutes; cargo checks crate hashes)" ;;
    android.target) echo "run      rustup target add $(cfg rust.android_targets)" ;;
    android.env)
      env_exports | while IFS= read -r l; do echo "env      $l"; done
      for f in $(profile_files); do echo "file     $f  (a delimited block that sources $(env_file); backup first)"; done ;;
    linux-cross.zig)
      dl_describe "$(dl_key zig)"
      echo "extract  to $(zig_dir)" ;;
    linux-cross.target) echo "run      rustup target add $(cfg rust.linux_target)" ;;
    linux-cross.env)
      env_exports | while IFS= read -r l; do case "$l" in *PISKA_ZIG*) echo "env      $l" ;; esac; done
      for f in $(profile_files); do echo "file     $f  (a delimited block that sources $(env_file); backup first)"; done ;;
    engine.source)
      if [ "$IN_CHECKOUT" = 1 ]; then echo "run      git -C \"$REPO_DIR\" submodule update --init --recursive   (the modeler is a submodule in modeler/; skipped when it is already there)"
      else echo "run      git clone --recurse-submodules --branch $(cfg project.branch) $(cfg project.repo) \"$REPO_DIR\"   (an existing clone: git pull --ff-only, then git submodule update --init --recursive)"; fi ;;
    engine.build) echo "run      cargo build --release $(for b in $(cfg project.binaries); do printf -- '-p %s ' "$b"; done)  (in $REPO_DIR; 20-40 minutes, about 3 GB)"
      echo "run      cargo build --release --manifest-path $(cfg project.modeler_manifest) -p $(cfg project.modeler_binary)   (in $REPO_DIR)" ;;
    engine.desktop)
      echo "file     $HOME/.local/share/applications/piska-hub.desktop, piska-editor.desktop, piska-modeler.desktop"
      echo "file     $HOME/.local/share/icons/hicolor/256x256/apps/piska-engine.png  (from assets/branding/icon-256.png)"
      [ "${DESKTOP_SHORTCUT:-0}" = 1 ] && echo "file     $(desktop_dir)/piska-hub.desktop  (desktop shortcut)" ;;
    engine.assoc) echo "file     $HOME/.local/share/mime/packages/piska-engine.xml (.pmodel .scene .scenario .$(cfg project.project_extension)) + update-mime-database, xdg-mime default (the project file opens the hub)" ;;
    engine.uninstaller) echo "copy     install.sh + packages.toml to $PREFIX  (uninstall.sh removes only what the manifest lists)" ;;
    config.preferences)
      prefs_values 0 | while IFS="$(printf '\t')" read -r k v; do echo "prefs    $k = $v"; done
      echo "file     $(prefs_path)  (merged: existing values are kept, a backup .piska-backup-<time> is made first)" ;;
    config.cargo)
      echo "file     ${CARGO_HOME:-$HOME/.cargo}/config.toml  (a delimited block is appended; backup first; existing content untouched)"
      echo "block    [build] / target-dir = \"$CARGO_TARGET_DIR_ARG\"" ;;
  esac
}

# --- the real work -------------------------------------------------------------------

run_cmd() { # run_cmd CMD ARGS... : logged, output teed to the log
  log_line "run: $*"
  "$@" 2>&1 | while IFS= read -r l; do if [ "$JSON" = 1 ]; then event log "$(jkv level output)$(jkn step "$CURRENT_STEP")$(jkv text "$l")"; else printf '%s\n' "$l"; fi; log_line "$l"; done
  return "${PIPESTATUS[0]}"
}

write_env_sh() {
  local f; f=$(env_file)
  mkdir -p "$(dirname "$f")"
  { echo "# Written by the Piska Engine installer. Safe to delete (uninstall removes it)."; env_exports; } > "$f"
  # JAVA_HOME placeholder -> real folder
  local j; j=$(jdk_home); [ -n "$j" ] && sed -i.bak "s#<JDK folder>#$j#" "$f" && rm -f "$f.bak"
  manifest_add file env.sh "$f" "" "$STEP_COMPONENT"
  local p text backup block
  block=". \"$f\""
  for p in $(profile_files); do
    text=""; [ -f "$p" ] && text=$(cat "$p")
    case "$text" in *"piska-engine installer: env"*) continue ;; esac
    backup=$(backup_file "$p")
    add_block "$text" env "$block" > "$p.new" && mv "$p.new" "$p"
    manifest_add block env "$p" "$backup" "$STEP_COMPONENT"
    say "  added a block to $p ${backup:+(backup: $backup)}"
  done
  say "  open a NEW terminal for the variables to take effect"
}

step_run() {
  local id="$1" sdk file tmp pkgs miss sdkm
  sdk=$(sdk_root)
  STEP_COMPONENT=${id%%.*}
  case "$id" in
    core.packages|cpp.packages)
      pkgs=$(pkgs_of_step "$id"); miss=$(missing_pkgs "$pkgs")
      if [ "$FAMILY" = macos ] && [ "$id" = core.packages ] && ! xcode-select -p >/dev/null 2>&1; then
        say "  Xcode Command Line Tools are missing: starting 'xcode-select --install' (a system dialog opens). Re-run this installer when it finishes."
        xcode-select --install || true; return "$EXIT_REBOOT"
      fi
      [ -n "$miss" ] && pkg_install_run "$miss" ;;
    android.jdk)
      if [ "$FAMILY" = macos ]; then
        command -v brew >/dev/null 2>&1 || { echo "error: Homebrew is not installed (https://brew.sh)" >&2; return 1; }
        # shellcheck disable=SC2086
        $(cfg macos.install_cask) $(cfg macos.jdk_cask) && manifest_add pkg jdk "$(cfg macos.jdk_cask)" "" android
      else pkg_install_run "$(missing_pkgs "$(pkgs_of_step android.jdk)")"; fi ;;
    core.rustup)
      if command -v rustup >/dev/null 2>&1; then run_cmd rustup update "$(cfg rust.toolchain)" || return 1
      else
        file=$(verified_file "$(dl_key rustup)") || return 1
        chmod +x "$file"
        run_cmd "$file" -y --default-toolchain "$(cfg rust.toolchain)" --profile minimal || return 1
        manifest_add rustup rustup "" "" core
      fi
      # shellcheck disable=SC1091
      [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env" ;;
    core.rust-components) run_cmd rustup component add $(cfg rust.components) || return 1 ;;
    core.vulkan) warn "libvulkan.so.1 was not found: install the Vulkan loader and a driver (Mesa or NVIDIA). This installer does not install GPU drivers." ;;
    web.target) run_cmd rustup target add "$(cfg rust.web_target)" || return 1; manifest_add rust-target "$(cfg rust.web_target)" "" "" web ;;
    android.cmdline-tools)
      if [ "$OS" = linux ] && [ "$ARCH" != x86_64 ]; then echo "error: the Android NDK has no Linux $ARCH host build; use an x86_64 machine for Android exports" >&2; return 1; fi
      file=$(verified_file "$(dl_key android)") || return 1
      tmp="$CACHE/cmdline-tools-extract"; rm -rf "$tmp"; mkdir -p "$tmp" "$sdk/cmdline-tools"
      unzip -q "$file" -d "$tmp" || return 1
      rm -rf "$sdk/cmdline-tools/latest"; mv "$tmp/cmdline-tools" "$sdk/cmdline-tools/latest" && rm -rf "$tmp"
      manifest_add dir android-cmdline-tools "$sdk/cmdline-tools/latest" "" android ;;
    android.licenses)
      export JAVA_HOME="${JAVA_HOME:-$(jdk_home)}"
      sdkm="$sdk/cmdline-tools/latest/bin/sdkmanager"
      yes 2>/dev/null | "$sdkm" --sdk_root="$sdk" --licenses >/dev/null 2>&1 || true
      [ -f "$sdk/licenses/android-sdk-license" ] || { echo "error: sdkmanager did not record the licence" >&2; return 1; } ;;
    android.packages)
      export JAVA_HOME="${JAVA_HOME:-$(jdk_home)}"
      sdkm="$sdk/cmdline-tools/latest/bin/sdkmanager"
      miss=$(missing_sdk_packages)
      # shellcheck disable=SC2086
      run_cmd "$sdkm" --sdk_root="$sdk" $miss || return 1
      manifest_add dir sdk-packages "$sdk" "$miss" android ;;
    android.cargo-ndk) run_cmd cargo install cargo-ndk --version "$(cfg android.cargo_ndk)" --locked || return 1; manifest_add cargo-crate cargo-ndk "" "" android ;;
    android.target) run_cmd rustup target add "$(cfg rust.android_targets)" || return 1; manifest_add rust-target "$(cfg rust.android_targets)" "" "" android ;;
    android.env|linux-cross.env) write_env_sh ;;
    linux-cross.zig)
      file=$(verified_file "$(dl_key zig)") || return 1
      mkdir -p "$(zig_dir)"
      tar -xJf "$file" -C "$(zig_dir)" || return 1
      [ -x "$(zig_bin)" ] || { echo "error: zig was not found in $(zig_dir) after extraction" >&2; return 1; }
      manifest_add dir zig "$(zig_dir)" "" linux-cross ;;
    linux-cross.target) run_cmd rustup target add "$(cfg rust.linux_target)" || return 1; manifest_add rust-target "$(cfg rust.linux_target)" "" "" linux-cross ;;
    engine.source)
      if [ "$IN_CHECKOUT" != 1 ]; then
        if [ -d "$REPO_DIR/.git" ]; then run_cmd git -C "$REPO_DIR" pull --ff-only || { echo "error: git pull failed: resolve it by hand in $REPO_DIR" >&2; return 1; }
        else mkdir -p "$(dirname "$REPO_DIR")"; run_cmd git clone --recurse-submodules --branch "$(cfg project.branch)" "$(cfg project.repo)" "$REPO_DIR" || { echo "error: git clone failed. The repository may be private: sign in to GitHub (gh auth login or a credential helper) and run again." >&2; return 1; }; manifest_add dir engine-source "$REPO_DIR" "" engine; fi
      fi
      run_cmd git -C "$REPO_DIR" submodule update --init --recursive || return 1 ;;
    engine.build)
      local args=""; for b in $(cfg project.binaries); do args="$args -p $b"; done
      # shellcheck disable=SC2086
      (cd "$REPO_DIR" && run_cmd cargo build --release $args) || return 1
      (cd "$REPO_DIR" && run_cmd cargo build --release --manifest-path "$(cfg project.modeler_manifest)" -p "$(cfg project.modeler_binary)") || return 1 ;;
    engine.desktop)
      local apps="piska-hub:Piska Hub piska-editor:Piska Editor piska-modeler:Piska Modeler" a bin name appdir iconfile
      appdir="$HOME/.local/share/applications"; iconfile="$HOME/.local/share/icons/hicolor/256x256/apps/piska-engine.png"
      mkdir -p "$appdir" "$(dirname "$iconfile")"
      cp "$REPO_DIR/assets/branding/icon-256.png" "$iconfile" && manifest_add file icon "$iconfile" "" engine
      for a in piska-hub:Piska\ Hub piska-editor:Piska\ Editor piska-modeler:Piska\ Modeler; do
        bin=${a%%:*}; name=${a#*:}
        local mime=""
        if [ "$FILE_ASSOC" = 1 ]; then case "$bin" in piska-hub) mime="application/x-piska-project;" ;; piska-editor) mime="application/x-piska-scene;application/x-piska-scenario;" ;; piska-modeler) mime="application/x-piska-model;" ;; esac; fi
        printf '[Desktop Entry]\nType=Application\nName=%s\nExec=%s%s\nPath=%s\nIcon=piska-engine\nTerminal=false\nCategories=Development;\n' "$name" "$(exe_path "$bin")" "$([ -n "$mime" ] && printf ' %%f')" "$REPO_DIR" > "$appdir/$bin.desktop"
        [ -n "$mime" ] && printf 'MimeType=%s\n' "$mime" >> "$appdir/$bin.desktop"
        manifest_add file "$bin.desktop" "$appdir/$bin.desktop" "" engine
      done
      if [ "$DESKTOP_SHORTCUT" = 1 ]; then
        mkdir -p "$(desktop_dir)" && cp "$appdir/piska-hub.desktop" "$(desktop_dir)/piska-hub.desktop" && chmod +x "$(desktop_dir)/piska-hub.desktop" && manifest_add file desktop-shortcut "$(desktop_dir)/piska-hub.desktop" "" engine
      fi
      command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$appdir" 2>/dev/null || true ;;
    engine.assoc)
      local x="$HOME/.local/share/mime/packages/piska-engine.xml"
      mkdir -p "$(dirname "$x")"
      {
        printf '<?xml version="1.0" encoding="UTF-8"?>\n<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">\n'
        printf '  <mime-type type="application/x-piska-model"><comment>Piska model</comment><glob pattern="*.pmodel"/></mime-type>\n'
        printf '  <mime-type type="application/x-piska-scene"><comment>Piska scene</comment><glob pattern="*.scene"/></mime-type>\n'
        printf '  <mime-type type="application/x-piska-scenario"><comment>Piska scenario</comment><glob pattern="*.scenario"/></mime-type>\n'
        printf '  <mime-type type="application/x-piska-project"><comment>%s</comment><glob pattern="*.%s"/></mime-type>\n' "$(cfg project.project_file_description)" "$(cfg project.project_extension)"
        printf '</mime-info>\n'
      } > "$x"
      manifest_add file mime "$x" "" engine
      command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$HOME/.local/share/mime" 2>/dev/null || true
      if command -v xdg-mime >/dev/null 2>&1; then
        xdg-mime default piska-hub.desktop application/x-piska-project 2>/dev/null || true
        xdg-mime default piska-editor.desktop application/x-piska-scene application/x-piska-scenario 2>/dev/null || true
        xdg-mime default piska-modeler.desktop application/x-piska-model 2>/dev/null || true
      fi ;;
    engine.uninstaller)
      mkdir -p "$PREFIX"
      cp "$SELF" "$PREFIX/install.sh" && cp "$DATA_FILE" "$PREFIX/packages.toml"
      printf '#!/bin/sh\nexec bash "$(dirname "$0")/install.sh" --uninstall "$@"\n' > "$PREFIX/uninstall.sh"; chmod +x "$PREFIX/uninstall.sh" "$PREFIX/install.sh"
      manifest_add dir installer-copy "$PREFIX" "" engine ;;
    config.preferences) update_preferences ;;
    config.cargo) update_cargo_config ;;
  esac
}

update_preferences() {
  local file vf text rep backup kept k
  file=$(prefs_path); vf=$(mktemp "${TMPDIR:-/tmp}/piska-vals.XXXXXX") || return 1; prefs_values 1 > "$vf"
  if [ ! -s "$vf" ]; then say "  no tool paths to write"; rm -f "$vf"; return 0; fi
  text=$(mktemp "${TMPDIR:-/tmp}/piska-ron.XXXXXX"); [ -f "$file" ] && cp "$file" "$text" || : > "$text"
  rep=$(mktemp "${TMPDIR:-/tmp}/piska-rep.XXXXXX")
  merge_prefs "$text" "$vf" > "$text.new" 2> "$rep" || { echo "error: cannot edit $file" >&2; rm -f "$vf" "$text" "$text.new" "$rep"; return 1; }
  kept=$(sed -n 's/^KEPT //p' "$rep" | tr '\n' ' ')
  local ow=""
  for k in $kept; do
    if confirm "editor.ron already sets $k. Replace it with the detected value?" n; then ow="$ow $k"; else say "  kept your $k"; fi
  done
  if [ -n "$ow" ]; then merge_prefs "$text" "$vf" $ow > "$text.new" 2> "$rep"; fi
  if ! grep -q '^SET ' "$rep"; then say "  preferences already up to date"; rm -f "$vf" "$text" "$text.new" "$rep"; return 0; fi
  backup=$(backup_file "$file")
  mkdir -p "$(dirname "$file")"; cp "$text.new" "$file"
  manifest_add prefs editor.ron "$file" "$backup" config
  say "  wrote $(sed -n 's/^SET //p' "$rep" | tr '\n' ' ')to $file ${backup:+(backup: $backup)}"
  rm -f "$vf" "$text" "$text.new" "$rep"
}

update_cargo_config() {
  local f="${CARGO_HOME:-$HOME/.cargo}/config.toml" text="" backup
  [ -f "$f" ] && text=$(cat "$f")
  case "$text" in *target-dir*=*) case "$text" in *"piska-engine installer: target-dir"*) ;; *) warn "$f already sets target-dir: left untouched"; return 0 ;; esac ;; esac
  confirm "Append a [build] target-dir block to $f?" y || { say "  skipped"; return 0; }
  backup=$(backup_file "$f"); mkdir -p "$(dirname "$f")"
  add_block "$text" target-dir "[build]
target-dir = \"$CARGO_TARGET_DIR_ARG\"" > "$f.new" && mv "$f.new" "$f"
  manifest_add cargo-config target-dir "$f" "$backup" config
}

# ---------------------------------------------------------------------------------------
# Plan / install / uninstall
# ---------------------------------------------------------------------------------------

plan_event() { # plan_event N TOTAL ID SKIP(0|1) REASON FOUND -> the "plan" event of one step
  local n=$1 total=$2 id=$3 skip=$4 reason=$5 found=$6 lic lic_json=null elev="" dls="" s d mb=0 size
  lic=$(step_licence "$id")
  [ -n "$lic" ] && lic_json="{\"id\":$(jn "$(step_licence_id "$id")"),\"name\":$(json_str "${lic%%|*}"),\"url\":$(json_str "${lic#*|}")}"
  step_needs_root "$id" && elev="administrator rights are needed for the package manager (sudo asks for your password; the command is shown first)"
  for s in $(step_download_sections "$id"); do
    d=$(download_json "$s"); [ -n "$d" ] || continue
    dls="$dls${dls:+,}$d"; size=$(cfg "$s.size_mb"); mb=$((mb + ${size:-0}))
  done
  event plan "$(jkr n "$n")$(jkr total "$total")$(jkv id "$id")$(jkv component "${id%%.*}")$(jkv title "$(step_title "$id")")$(jkr skip "$(jbool [ "$skip" = 1 ])")$(jkn reason "$([ "$skip" = 1 ] && printf '%s' "$reason")")$(jkn found "$found")$(jkn version "$(found_version "$found")")$(jkn path "$(found_path "$found")")$(jkn elevation "$elev")$(jkr licence "$lic_json")$(jkr skippable "$(jbool in_list "$id" "$SKIPPABLE_STEPS")")$(jkr download_mb "$mb")$(jkr downloads "[$dls]")$(jkr lines "$(step_plan "$id" | plan_lines_json)")"
}

show_plan() {
  local ids n=0 total id found title requested reason
  ids=$(build_steps "$COMPONENTS"); total=$(printf '%s\n' "$ids" | grep -c .)
  for id in $ids; do
    n=$((n + 1)); title=$(step_title "$id"); CURRENT_STEP=$id
    if found=$(step_detect "$id") && [ "$ASSUME_CLEAN" != 1 ]; then detected=1; else detected=0; found=""; fi
    requested=0; skip_requested "$id" && requested=1
    if [ "$requested" = 1 ]; then reason=requested; elif [ "$detected" = 1 ]; then reason=found; else reason=done-earlier; fi
    if [ "$requested" = 1 ] || step_skip "$MODE" "$STATE_FILE" "$id" "$detected"; then
      if [ "$JSON" = 1 ]; then plan_event "$n" "$total" "$id" 1 "$reason" "$found"; continue; fi
      if [ "$requested" = 1 ]; then say "[$n/$total] SKIP $id - $title (skipped on request)"
      elif [ -n "$found" ]; then say "[$n/$total] SKIP $id - $title (found: $found)"; else say "[$n/$total] SKIP $id - $title (done earlier)"; fi
      continue
    fi
    if [ "$JSON" = 1 ]; then plan_event "$n" "$total" "$id" 0 "" "$found"; continue; fi
    say "[$n/$total] STEP $id - $title"
    step_needs_root "$id" && say "         elevation: administrator rights are needed for the package manager (sudo asks for your password; the command is shown first)"
    local lic; lic=$(step_licence "$id"); [ -n "$lic" ] && say "         licence: ${lic%%|*} (${lic#*|}) - accepted only with --accept-licenses or your answer"
    step_plan "$id" | while IFS= read -r l; do say "         $l"; done
  done
  CURRENT_STEP=""
}

start_install() {
  local ids n=0 total id found detected title lic rc errf
  STAT_TOTAL=$(build_steps "$COMPONENTS" | grep -c .)
  [ "$DRY_RUN" = 1 ] || LOG_FILE="$STATE_DIR/logs/install-$(date +%Y%m%d-%H%M%S).log"
  [ "$JSON" = 1 ] && write_hello
  if [ "$DRY_RUN" = 1 ]; then
    say "Components: $COMPONENTS"
    say "DRY RUN: nothing is installed, downloaded or changed. Detection below only reads."
    show_plan
    say "DRY RUN finished: no changes were made."
    return 0
  fi
  mkdir -p "$STATE_DIR/logs"
  say "Components: $COMPONENTS"; say "Log: $LOG_FILE"
  ids=$(build_steps "$COMPONENTS"); total=$(printf '%s\n' "$ids" | grep -c .)
  printf '%s\n' "$COMPONENTS" > "$STATE_DIR/components"
  for id in $ids; do
    n=$((n + 1)); title=$(step_title "$id"); CURRENT_STEP=$id
    if found=$(step_detect "$id"); then detected=1; else detected=0; found=""; fi
    if skip_requested "$id"; then
      say "[$n/$total] skip $title (skipped on request)"
      [ "$JSON" = 1 ] && event step "$(jkv phase skip)$(jkv step "$id")$(jkr n "$n")$(jkr total "$total")$(jkv title "$title")$(jkv reason requested)$(jkr found null)"
      state_set "$id" skipped; STAT_SKIPPED=$((STAT_SKIPPED + 1)); continue
    fi
    if step_skip "$MODE" "$STATE_FILE" "$id" "$detected"; then
      say "[$n/$total] skip $title (${found:-done earlier})"
      [ "$JSON" = 1 ] && event step "$(jkv phase skip)$(jkv step "$id")$(jkr n "$n")$(jkr total "$total")$(jkv title "$title")$(jkv reason "$([ "$detected" = 1 ] && echo found || echo done-earlier)")$(jkn found "$found")"
      state_set "$id" done; STAT_SKIPPED=$((STAT_SKIPPED + 1)); continue
    fi
    say "[$n/$total] $title"
    [ "$JSON" = 1 ] && event step "$(jkv phase start)$(jkv step "$id")$(jkr n "$n")$(jkr total "$total")$(jkv title "$title")$(jkv component "${id%%.*}")"
    lic=$(step_licence "$id")
    if [ -n "$lic" ]; then
      if ! confirm_license "${lic%%|*}" "${lic#*|}" "$(step_licence_id "$id")"; then
        say "  licence not accepted: stopping."
        [ "$JSON" = 1 ] && event step "$(jkv phase declined)$(jkv step "$id")$(jkv title "$title")$(jkv reason licence)$(jkn licence "$(step_licence_id "$id")")"
        return "$EXIT_DECLINED"
      fi
      [ "$JSON" = 1 ] && event licence "$(jkv step "$id")$(jkn licence "$(step_licence_id "$id")")$(jkv name "${lic%%|*}")$(jkv url "${lic#*|}")$(jkr accepted true)"
    fi
    if step_needs_root "$id" && ! { [ "$JSON" = 1 ] && [ "$ELEVATE" != sudo ]; }; then
      say "  This step runs the package manager as administrator. The commands:"
      step_plan "$id" | grep '^run' | while IFS= read -r l; do say "    $l"; done
      if [ "$CONFIRM_ELEVATION" != 1 ]; then confirm "  Continue?" y elevation "$id" || { say "  declined: stopping."; [ "$JSON" = 1 ] && event step "$(jkv phase declined)$(jkv step "$id")$(jkv title "$title")$(jkv reason elevation)$(jkr licence null)"; return "$EXIT_DECLINED"; }; fi
    fi
    if [ "$JSON" = 1 ]; then
      errf=$(mktemp "${TMPDIR:-/tmp}/piska-step.XXXXXX")
      step_run "$id" 2> "$errf"; rc=$?
      while IFS= read -r l; do [ -n "$l" ] && event log "$(jkv level error)$(jkv step "$id")$(jkv text "$l")"; done < "$errf"
      FAILED_ERROR=$(grep . "$errf" | tail -n 1); rm -f "$errf"
    else
      step_run "$id"; rc=$?
    fi
    if [ "$rc" -ne 0 ]; then
      state_set "$id" failed "exit $rc"
      if [ "$rc" = "$EXIT_REBOOT" ]; then
        REBOOT_NEEDED=1; FAILED_ERROR=""
        [ "$JSON" = 1 ] && event reboot "$(jkv step "$id")$(jkv title "$title")"
        say "Finish the system dialog, then run: install.sh --resume"; return "$EXIT_REBOOT"
      fi
      STAT_FAILED=$((STAT_FAILED + 1)); FAILED_STEP=$id; [ -n "$FAILED_ERROR" ] || FAILED_ERROR="exit $rc"
      if [ "$JSON" = 1 ]; then
        event step "$(jkv phase failed)$(jkv step "$id")$(jkr n "$n")$(jkr total "$total")$(jkv title "$title")$(jkv error "$FAILED_ERROR")$(jkn log "$LOG_FILE")$(jkr skippable "$(jbool in_list "$id" "$SKIPPABLE_STEPS")")$(jkr elevation_refused false)$(jkn manual_command "$MANUAL_COMMAND")"
      else printf 'FAILED step %s\n' "$id" >&2; fi
      log_line "FAILED step $id"
      say "Fix the problem and run again with --resume (steps that are done are skipped). Log: $LOG_FILE"
      return "$EXIT_FAILED"
    fi
    FAILED_ERROR=""
    state_set "$id" done; STAT_DONE=$((STAT_DONE + 1))
    [ "$JSON" = 1 ] && event step "$(jkv phase done)$(jkv step "$id")$(jkr n "$n")$(jkr total "$total")$(jkv title "$title")"
  done
  CURRENT_STEP=""
  say "All steps finished."
  run_doctor
  say "Open a NEW terminal so that PATH and the environment variables above take effect."
  return 0
}

run_doctor() {
  local exe list; exe=$(exe_path piska-export)
  if [ ! -x "$exe" ]; then say "Diagnostics: build the engine component, then run 'piska-export doctor' (or 'cargo run -p piska-export -- doctor')."; return 0; fi
  list=$(printf '%s\n' "$COMPONENTS" | tr ' ' '\n' | grep -vx engine | tr '\n' ',' | sed 's/,$//')
  if [ "$JSON" = 1 ]; then if [ -n "$list" ]; then run_cmd "$exe" doctor --components "$list" || true; else run_cmd "$exe" doctor || true; fi; return 0; fi
  if [ -n "$list" ]; then "$exe" doctor --components "$list" || true; else "$exe" doctor || true; fi
}

start_uninstall() {
  local plan action target ask
  if [ ! -s "$MANIFEST_FILE" ]; then
    [ "$JSON" = 1 ] && event uninstall-plan "$(jkv manifest "$MANIFEST_FILE")$(jkr dry_run "$(jbool [ "$DRY_RUN" = 1 ])")$(jkr items '[]')"
    say "Nothing to remove: no install manifest at $MANIFEST_FILE."; return 0
  fi
  plan=$(uninstall_plan "$MANIFEST_FILE")
  if [ "$JSON" = 1 ]; then
    local items=""
    while IFS="$(printf '\t')" read -r action target ask; do
      [ -n "$action" ] && items="$items${items:+,}{\"action\":$(json_str "$action"),\"target\":$(json_str "$target"),\"ask\":$(jbool [ "$ask" = 1 ])}"
    done <<EOF_PLAN
$plan
EOF_PLAN
    event uninstall-plan "$(jkv manifest "$MANIFEST_FILE")$(jkr dry_run "$(jbool [ "$DRY_RUN" = 1 ])")$(jkr items "[$items]")"
  fi
  say "Uninstall plan (only what this installer added; programs that were already on the machine are not listed):"
  printf '%s\n' "$plan" | while IFS="$(printf '\t')" read -r action target ask; do say "  $(printf '%-22s' "$action") $target$([ "$ask" = 1 ] && printf '   [asks first]')"; done
  [ "$DRY_RUN" = 1 ] && { say "dry run: nothing was changed."; return 0; }
  confirm "Proceed?" n uninstall-proceed || return "$EXIT_DECLINED"
  printf '%s\n' "$plan" | while IFS="$(printf '\t')" read -r action target ask; do
    if [ "$ask" = 1 ]; then
      if [ "$JSON" = 1 ]; then confirm "Remove $target ($action)?" n uninstall-item "$target" || { say "  kept $target"; continue; }
      elif ! confirm "Remove $target ($action)?" n </dev/tty 2>/dev/null; then say "  kept $target"; continue; fi
    fi
    case "$action" in
      delete-file) rm -f "$target" ;;
      delete-dir) rm -rf "$target" ;;
      cargo-uninstall) cargo uninstall "$target" ;;
      rustup-target-remove) rustup target remove "$target" ;;
      rustup-self-uninstall) rustup self uninstall -y ;;
      remove-block) f=${target%% (*}; id=${target##*(}; id=${id%)}; remove_block "$f" "$id" > "$f.new" && mv "$f.new" "$f" ;;
      restore-prefs) b=$(awk -F'\t' -v f="$target" '$1 == "prefs" && $3 == f { print $4 }' "$MANIFEST_FILE" | tail -n 1); [ -n "$b" ] && [ -f "$b" ] && cp "$b" "$target" ;;
      note-packages) : ;;
    esac
    say "  done: $action $target"
  done
  rm -f "$MANIFEST_FILE" "$STATE_FILE"
  return 0
}

# ---------------------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------------------

usage() { sed -n '2,12p' "$SELF" | sed 's/^# \{0,1\}//'; }

main() {
  YES=0; DRY_RUN=0; ASSUME_CLEAN=0; ACCEPT_LICENSES=0; NO_PREFS=0; FILE_ASSOC=0; MODE=normal; UNINSTALL=0; LIST=0
  JSON=0; ACCEPT_LICENSE=""; CONFIRM_ELEVATION=0; SKIP_STEPS=""; SOURCE_DIR=""; ELEVATE=""; NO_SHORTCUTS=0; DESKTOP_SHORTCUT=0
  PREFIX=""; CACHE=""; ANDROID_SDK_ARG=""; BASE_URL=""; COMPONENTS_ARG=""; ASSUME_OS=""; ASSUME_ARCH=""; CARGO_TARGET_DIR_ARG=""; SKIP_PACKAGES=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --components) COMPONENTS_ARG=${2:?--components needs a value}; shift ;;
      --yes|-y) YES=1 ;;
      --dry-run) DRY_RUN=1 ;;
      --assume-clean) ASSUME_CLEAN=1; DRY_RUN=1 ;;
      --assume-os) ASSUME_OS=${2:?}; shift ;;
      --assume-arch) ASSUME_ARCH=${2:?}; shift ;;
      --prefix) PREFIX=${2:?--prefix needs a value}; shift ;;
      --cache) CACHE=${2:?}; shift ;;
      --android-sdk) ANDROID_SDK_ARG=${2:?}; shift ;;
      --base-url) BASE_URL=${2:?}; shift ;;
      --accept-licenses) ACCEPT_LICENSES=1 ;;
      --resume) MODE=resume ;;
      --repair) MODE=repair ;;
      --uninstall) UNINSTALL=1 ;;
      --no-prefs) NO_PREFS=1 ;;
      --file-associations) FILE_ASSOC=1 ;;
      --cargo-target-dir) CARGO_TARGET_DIR_ARG=${2:?}; shift ;;
      --skip-packages) SKIP_PACKAGES=1 ;;
      --list-components) LIST=1 ;;
      --json) JSON=1; exec 3>&1 4<&0 ;;
      --accept-license) ACCEPT_LICENSE=${2:?--accept-license needs a value}; shift ;;
      --confirm-elevation) CONFIRM_ELEVATION=1 ;;
      --skip-steps) SKIP_STEPS=${2:?--skip-steps needs a value}; shift ;;
      --source) SOURCE_DIR=${2:?--source needs a value}; shift ;;
      --elevate) ELEVATE=${2:?--elevate needs pkexec, sudo or none}; shift ;;
      --no-shortcuts) NO_SHORTCUTS=1 ;;
      --desktop-shortcut) DESKTOP_SHORTCUT=1 ;;
      --help|-h) usage; return 0 ;;
      *) echo "error: unknown option '$1' (see --help)" >&2; return "$EXIT_BAD_ARGS" ;;
    esac
    shift
  done
  if [ "$JSON" = 1 ]; then
    # Events go to fd 3 (the real stdout, also from inside $(...)); answers are read from fd 4 (the real stdin):
    # both are opened where --json is parsed.
    if [ -z "$ELEVATE" ]; then if command -v pkexec >/dev/null 2>&1; then ELEVATE=pkexec; else ELEVATE=none; fi; fi
  fi
  case "${ELEVATE:-sudo}" in pkexec|sudo|none) ;; *) echo "error: --elevate takes pkexec, sudo or none" >&2; return "$EXIT_BAD_ARGS" ;; esac
  [ -n "$ELEVATE" ] || ELEVATE=sudo

  # data file: next to the script, else downloaded from the repository
  if [ -f "$SCRIPT_DIR/packages.toml" ]; then DATA_FILE="$SCRIPT_DIR/packages.toml"
  else
    local base="${BASE_URL:-https://raw.githubusercontent.com/boobser1488/piska-engine/main/install}"
    DATA_FILE="${TMPDIR:-/tmp}/piska-packages.$$.toml"
    echo "No local checkout: fetching $base/packages.toml"
    curl_get "$base/packages.toml" "$DATA_FILE" || { echo "error: cannot fetch packages.toml" >&2; return "$EXIT_FAILED"; }
    echo "packages.toml sha256: $(sha256_of "$DATA_FILE")  (compare with the repository if you want to be sure)"
  fi
  DATA=$(toml_flatten "$DATA_FILE")

  detect_platform
  [ -n "$PREFIX" ] || PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}/$(cfg project.install_dir_name)"
  STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/$(cfg project.state_dir_name)/install"
  STATE_FILE="$STATE_DIR/state.tsv"; MANIFEST_FILE="$STATE_DIR/manifest.tsv"; LOG_FILE=""
  [ -n "$CACHE" ] || CACHE="$PREFIX/cache"
  IN_CHECKOUT=0; REPO_DIR="$PREFIX/engine"
  if [ -f "$SCRIPT_DIR/../Cargo.toml" ] && [ -d "$SCRIPT_DIR/../tools/piska-export" ]; then IN_CHECKOUT=1; REPO_DIR=$(cd "$SCRIPT_DIR/.." && pwd); fi
  # --source: an existing checkout chosen by the user (the graphical installer runs this script from a temp folder).
  if [ -n "$SOURCE_DIR" ]; then
    if [ -f "$SOURCE_DIR/Cargo.toml" ] && [ -d "$SOURCE_DIR/tools/piska-export" ]; then IN_CHECKOUT=1; REPO_DIR=$(cd "$SOURCE_DIR" && pwd)
    else
      if [ "$JSON" = 1 ]; then event error "$(jkv text "--source $SOURCE_DIR is not an engine checkout (no Cargo.toml and tools/piska-export there)")"
      else echo "error: --source $SOURCE_DIR is not an engine checkout (no Cargo.toml and tools/piska-export there)" >&2; fi
      return "$EXIT_BAD_ARGS"
    fi
  fi

  if [ "$LIST" = 1 ]; then
    local c; for c in $ALL_COMPONENTS; do printf '%-12s %-8s %s\n' "$c" "$(cfg "component.$c.size_unix")" "$(cfg "component.$c.title")"; done
    return 0
  fi
  if [ "$UNINSTALL" = 1 ]; then start_uninstall; return $?; fi

  local requested="$COMPONENTS_ARG"
  if [ -z "$requested" ] && [ "$MODE" != normal ] && [ -s "$STATE_DIR/components" ]; then requested=$(cat "$STATE_DIR/components"); fi
  if [ -z "$requested" ]; then
    if [ "$DRY_RUN" = 1 ]; then requested=all
    elif [ "$YES" = 1 ]; then requested="core engine"; say "No --components given: using core,engine."
    elif [ "$JSON" = 1 ]; then event error "$(jkv text "--json needs --components (or --dry-run / --yes / --resume)")"; return "$EXIT_BAD_ARGS"
    else requested=$(show_menu) || return "$EXIT_BAD_ARGS"; fi
  fi
  if [ "$JSON" = 1 ]; then
    COMPONENTS=$(resolve_components "$requested" 2>&1) || { event error "$(jkv text "$COMPONENTS")"; return "$EXIT_BAD_ARGS"; }
  else COMPONENTS=$(resolve_components "$requested") || return "$EXIT_BAD_ARGS"; fi
  start_install
}

show_menu() {
  local i=0 c ans out="" n
  {
    echo ""; echo "Piska Engine installer - choose components (dependencies are added automatically):"
    for c in $ALL_COMPONENTS; do i=$((i + 1)); printf '  %d) %-12s %s  [%s]\n' "$i" "$c" "$(cfg "component.$c.title")" "$(cfg "component.$c.size_unix")"; done
  } >&2
  printf 'Numbers or names separated by commas, "a" for all, Enter for core + engine: ' >&2
  read -r ans || return 1
  [ -z "$ans" ] && { echo "core engine"; return 0; }
  case "$ans" in a|all) echo all; return 0 ;; esac
  for n in $(printf '%s' "$ans" | tr ',' ' '); do
    case "$n" in
      [0-9]*) i=0; for c in $ALL_COMPONENTS; do i=$((i + 1)); [ "$i" = "$n" ] && out="$out $c"; done ;;
      *) out="$out $n" ;;
    esac
  done
  echo "$out"
}

SELF="${BASH_SOURCE[0]:-$0}"
SCRIPT_DIR=$(cd "$(dirname "$SELF")" 2>/dev/null && pwd || echo "")
DATA=""; DRY_RUN=0; YES=0; ASSUME_CLEAN=0; LOG_FILE=""

# Tests source this file with PISKA_INSTALL_NOMAIN=1 to reach the functions above.
if [ -z "${PISKA_INSTALL_NOMAIN:-}" ]; then
  main "$@"
  rc=$?
  # --json: the stream always ends with a "summary" event.
  [ "$JSON" = 1 ] && write_summary "$rc"
  exit "$rc"
fi
