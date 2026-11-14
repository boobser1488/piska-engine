#!/usr/bin/env bash
# Self-tests of install/install.sh. Nothing is installed, downloaded or changed: pure functions,
# syntax checks, checksum checks on local fixtures, and dry-run golden files.
#
#   bash install/tests/run.sh                  # run
#   bash install/tests/run.sh --update-golden  # rewrite install/tests/golden/unix-*.txt (review the diff!)

set -u
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
INSTALL=$(cd "$HERE/.." && pwd)
REPO=$(cd "$INSTALL/.." && pwd)
GOLDEN="$HERE/golden"
UPDATE=0; [ "${1:-}" = "--update-golden" ] && UPDATE=1
PASSED=0; FAILED=0
TMPD=$(mktemp -d "${TMPDIR:-/tmp}/piska-install-tests.XXXXXX")
trap 'rm -rf "$TMPD"' EXIT

check() { # check NAME CMD...  (the command runs in a subshell; non-zero = failure)
  local name="$1"; shift
  local out
  if out=$("$@" 2>&1); then PASSED=$((PASSED + 1)); echo "ok   $name"
  else FAILED=$((FAILED + 1)); echo "FAIL $name"; printf '%s\n' "$out" | head -n 12 | sed 's/^/     /'; fi
}
eq() { [ "$1" = "$2" ] || { echo "expected '$2', got '$1'" >&2; return 1; }; }

# ---- syntax ---------------------------------------------------------------------------
check "bash -n install.sh" bash -n "$INSTALL/install.sh"
check "bash -n run.sh" bash -n "$HERE/run.sh"
check "bash -n verify-pins.sh" bash -n "$HERE/verify-pins.sh"
check "install.sh is plain ASCII with LF line endings" bash -c "! LC_ALL=C grep -q \$'\\r' '$INSTALL/install.sh' && ! LC_ALL=C grep -q '[^ -~	]' '$INSTALL/install.sh'"

PISKA_INSTALL_NOMAIN=1
export PISKA_INSTALL_NOMAIN
# shellcheck disable=SC1091
. "$INSTALL/install.sh"
set +e   # the sourced script enables -u only; keep going after a failed check
DATA=$(toml_flatten "$INSTALL/packages.toml")

# ---- data file --------------------------------------------------------------------------
check "packages.toml has the sections the installer needs" bash -c '
  . "$0"; DATA=$(toml_flatten "$(dirname "$0")/packages.toml"); for k in project.repo min.rust rust.web_target android.ndk zig.version component.core.title linux.debian.build macos.install; do [ -n "$(cfg $k)" ] || { echo "missing $k"; exit 1; }; done' "$INSTALL/install.sh"
sections() { printf '%s\n' "$DATA" | awk -F'\t' '{ n = $1; sub(/\.[^.]*$/, "", n); print n }' | grep '^download\.' | sort -u; }
data_policy() {
  local n=0 s
  for s in $(sections); do
    assert_download "$s" || return 1; n=$((n + 1))
  done
  [ "$n" -ge 10 ] || { echo "only $n downloads"; return 1; }
}
check "every download has an https URL and a pinned sha256 or a sha256_url" data_policy
refuse_without_checksum() {
  DATA="download.x.url	https://example.com/x.zip"
  assert_download download.x 2>/dev/null && return 1
  DATA="download.x.url	http://example.com/x.zip
download.x.sha256	$(printf 'a%.0s' $(seq 64))"
  assert_download download.x 2>/dev/null && return 1
  return 0
}
check "a download entry without a checksum (or over http) is refused" refuse_without_checksum
DATA=$(toml_flatten "$INSTALL/packages.toml")
zig_versions() {
  local s v; v=$(cfg zig.version)
  for s in $(sections | grep zig_); do case "$(cfg "$s.url")" in */$v/*) ;; *) echo "$s url has no $v"; return 1 ;; esac; done
  case " $(cfg android.sdk_packages) " in *" platforms;$(cfg android.platform) "*) ;; *) echo platform; return 1 ;; esac
  case " $(cfg android.sdk_packages) " in *" build-tools;$(cfg android.build_tools) "*) ;; *) echo build-tools; return 1 ;; esac
  case " $(cfg android.sdk_packages) " in *" ndk;$(cfg android.ndk) "*) ;; *) echo ndk; return 1 ;; esac
}
check "zig URLs carry the pinned version; SDK packages match the version keys" zig_versions
check "versions match what piska-export suggests (android.rs)" bash -c "grep -q '$(cfg android.ndk)' '$REPO/tools/piska-export/src/android.rs' && grep -q '$(cfg android.build_tools)' '$REPO/tools/piska-export/src/android.rs'"
no_hardcoding() {
  local f needle
  for f in "$INSTALL/install.sh" "$INSTALL/install.ps1"; do
    for needle in commandlinetools- ziglang.org/download/0 static.rust-lang.org/rustup "$(cfg android.ndk)" "$(cfg android.build_tools)" "cargo-ndk $(cfg android.cargo_ndk)" "$(cfg zig.version)"; do
      if grep -qF -- "$needle" "$f"; then echo "$(basename "$f") hard-codes '$needle'"; return 1; fi
    done
  done
}
check "URLs and versions live only in packages.toml" no_hardcoding
docs_match() {
  local doc="$REPO/docs/install.md" fam kind p c
  for p in $(cfg zig.version) $(cfg android.ndk) $(cfg android.build_tools) $(cfg android.platform) $(cfg android.cargo_ndk) $(cfg rust.web_target) $(cfg winget.buildtools) $(cfg winget.jdk); do grep -qF -- "$p" "$doc" || { echo "install.md lacks $p"; return 1; }; done
  for c in core cpp web android linux-cross engine; do
    for p in "$(cfg component.$c.size_windows)" "$(cfg component.$c.size_unix)"; do grep -qF -- "$p" "$doc" || { echo "install.md lacks size '$p' of $c"; return 1; }; done
  done
  for fam in debian fedora arch suse; do for kind in build run cpp jdk; do for p in $(cfg linux.$fam.$kind); do grep -qF -- "$p" "$doc" || { echo "install.md lacks $fam package $p"; return 1; }; done; done; done
  for fam in debian fedora arch; do for p in $(cfg linux.$fam.build); do grep -qF -- "$p" "$REPO/docs/api/export.md" || { echo "export.md lacks $fam package $p"; return 1; }; done; done
}
check "docs/install.md states every version, size and package; export.md lists the build packages" docs_match

# ---- pure functions ------------------------------------------------------------------------
check "version_ge" bash -c '. "$0"; version_ge 1.97.1 1.86 && version_ge 3.20 3.20.0 && ! version_ge 1.85.9 1.86 && version_ge 0.17.1 0.13 && ! version_ge 0.9 0.13' "$INSTALL/install.sh"
check "first_version" bash -c '. "$0"; [ "$(first_version "rustc 1.97.1 (8bab26f4f 2026-07-14)")" = 1.97.1 ] && [ "$(first_version "cmake version 3.31.0-rc2")" = 3.31.0 ] && [ "$(first_version "0.17.1")" = 0.17.1 ] && [ -z "$(first_version "no digits")" ]' "$INSTALL/install.sh"
fixtures_checksum() {
  local f="$TMPD/fixture.bin" good
  printf 'piska fixture' > "$f"; good=$(sha256_of "$f")
  checksum_ok "$f" "$good" || { echo "good file rejected"; return 1; }
  checksum_ok "$f" "$(printf '%s' "$good" | tr a-f A-F)" || { echo "upper case rejected"; return 1; }
  printf 'x' >> "$f"
  checksum_ok "$f" "$good" && { echo "TAMPERED FILE ACCEPTED"; return 1; }
  checksum_ok "$f" abc && { echo "short hash accepted"; return 1; }
  checksum_ok "$TMPD/missing" "$good" && { echo "missing file accepted"; return 1; }
  return 0
}
check "checksum verification: good, tampered, malformed, missing" fixtures_checksum
cached_reuse() {
  # a cached file that passes its checksum is reused with no network (curl is replaced by a failing stub)
  local f="$TMPD/cache/tool.zip" sum
  mkdir -p "$TMPD/cache"; printf 'cached tool' > "$f"; sum=$(sha256_of "$f")
  DATA="download.t.url	https://example.invalid/dl/tool.zip
download.t.sha256	$sum"
  CACHE="$TMPD/cache"
  curl() { echo "NETWORK USED" >&2; return 1; }
  local got; got=$(verified_file download.t 2>/dev/null) || return 1
  unset -f curl
  eq "$got" "$f"
}
check "a cached file that passes its checksum is reused without network access" cached_reuse
DATA=$(toml_flatten "$INSTALL/packages.toml")
check "checksum text and zig index.json parsing" bash -c '
  . "$0"; h=$(printf "a%.0s" $(seq 64)); [ "$(sha_from_text "$h  rustup-init")" = "$h" ] && [ -z "$(sha_from_text "nothing")" ] &&
  json="{\"0.17.1\":{\"x86_64-linux\":{
    \"tarball\": \"https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.1.tar.xz\",
    \"shasum\": \"bbbb\",
    \"size\": \"1\" }}}" &&
  [ "$(zig_index_sha "$json" https://ziglang.org/download/0.17.1/zig-x86_64-linux-0.17.1.tar.xz)" = bbbb ] &&
  [ -z "$(zig_index_sha "$json" https://ziglang.org/other.tar.xz)" ]' "$INSTALL/install.sh"
os_fixtures() {
  local d="$TMPD/osrel"; mkdir -p "$d"
  printf 'ID=ubuntu\nID_LIKE=debian\n' > "$d/ubuntu"
  printf 'ID=linuxmint\nID_LIKE="ubuntu debian"\n' > "$d/mint"
  printf 'ID=fedora\n' > "$d/fedora"
  printf 'ID=arch\n' > "$d/arch"
  printf 'ID=manjaro\nID_LIKE=arch\n' > "$d/manjaro"
  printf 'ID="opensuse-leap"\nID_LIKE="suse opensuse"\n' > "$d/suse"
  printf 'ID=plan9\n' > "$d/unknown"
  eq "$(os_family "$d/ubuntu")" debian && eq "$(os_family "$d/mint")" debian && eq "$(os_family "$d/fedora")" fedora &&
  eq "$(os_family "$d/arch")" arch && eq "$(os_family "$d/manjaro")" arch && eq "$(os_family "$d/suse")" suse &&
  eq "$(os_family "$d/unknown")" "" && eq "$(os_family "$d/nonexistent")" ""
}
check "distro detection from os-release fixtures" os_fixtures
check "resolve_components adds dependencies, keeps order, rejects unknown names" bash -c '
  . "$0"; DATA=$(toml_flatten "$1"); [ "$(resolve_components web)" = "core web" ] && [ "$(resolve_components engine,android)" = "core android engine" ] &&
  [ "$(resolve_components all)" = "core cpp web android linux-cross engine" ] && ! resolve_components nope 2>/dev/null' "$INSTALL/install.sh" "$INSTALL/packages.toml"
prefs_merge() {
  local t="$TMPD/editor.ron" v="$TMPD/values.tsv" out rep
  printf '(\n    ui_scale: 1.25,\n    tool_cargo: /mine/cargo,\n    tool_zig: ,\n    undo_limit: 100,\n)\n' > "$t"
  printf 'tool_cargo\t/other/cargo\ntool_zig\t/zig/zig\ntool_java\t/jdk%%2C17\n' > "$v"
  out=$(merge_prefs "$t" "$v" 2> "$TMPD/rep") || return 1
  rep=$(sort "$TMPD/rep" | tr '\n' ' ')
  eq "$rep" "KEPT tool_cargo SET tool_java SET tool_zig " || return 1
  printf '%s\n' "$out" | grep -q 'tool_cargo: /mine/cargo,' || { echo "user value lost"; return 1; }
  printf '%s\n' "$out" | grep -q 'tool_zig: /zig/zig,' || { echo "empty value not filled"; return 1; }
  printf '%s\n' "$out" | grep -q 'tool_java: /jdk%2C17,' || { echo "key not added"; return 1; }
  printf '%s\n' "$out" | grep -q 'ui_scale: 1.25,' || { echo "other keys changed"; return 1; }
  [ "$(printf '%s\n' "$out" | tail -n 1)" = ")" ] || { echo "not closed"; return 1; }
  out=$(merge_prefs "$t" "$v" tool_cargo 2>/dev/null); printf '%s\n' "$out" | grep -q 'tool_cargo: /other/cargo,' || { echo "overwrite on request failed"; return 1; }
  printf '%s\n' "$(merge_prefs "$t" "$v" 2>/dev/null)" > "$TMPD/merged"
  merge_prefs "$TMPD/merged" "$v" 2> "$TMPD/rep2" > /dev/null; grep -q '^SET' "$TMPD/rep2" && { echo "second merge still changes things"; return 1; }
  : > "$TMPD/empty"; out=$(merge_prefs "$TMPD/empty" "$v" 2>/dev/null)
  printf '%s\n' "$out" | head -n 1 | grep -qx '(' || { echo "new file not created as ( ... )"; return 1; }
  printf 'garbage' > "$TMPD/bad"; merge_prefs "$TMPD/bad" "$v" >/dev/null 2>&1 && { echo "broken file accepted"; return 1; }
  return 0
}
check "preferences merge: add, fill empty, keep user values, idempotent, refuses broken files" prefs_merge
blocks() {
  local orig=$'[net]\ngit-fetch-with-cli = true' new
  new=$(add_block "$orig" env 'export A=1')
  case "$new" in "$orig"*) ;; *) echo "existing content moved"; return 1 ;; esac
  case "$new" in *'export A=1'*) ;; *) echo "block missing"; return 1 ;; esac
  eq "$(add_block "$new" env x)" "$new" || return 1
  printf '%s\n' "$new" > "$TMPD/blockfile"
  eq "$(remove_block "$TMPD/blockfile" env | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}')" "$orig" || return 1
}
check "delimited blocks: added once, existing text untouched, removable" blocks
uninstall_from_manifest() {
  local m="$TMPD/manifest.tsv" plan
  printf 'pkg\tpackages\tbuild-essential\t\tcore\tt\nrust-target\twasm32-unknown-unknown\t\t\tweb\tt\nblock\tenv\t/h/.profile\t/h/.profile.bak\tandroid\tt\nfile\tpiska-hub.desktop\t/h/a.desktop\t\tengine\tt\nprefs\teditor.ron\t/h/editor.ron\t/h/editor.ron.bak\tconfig\tt\n' > "$m"
  plan=$(uninstall_plan "$m" | cut -f1 | tr '\n' ',')
  eq "$plan" "restore-prefs,delete-file,remove-block,rustup-target-remove,note-packages," || return 1
  eq "$(uninstall_plan "$m" | awk -F'\t' '$1 == "restore-prefs" { print $3 }')" 1 || return 1
  eq "$(uninstall_plan "$TMPD/nothing" | wc -l | tr -d ' ')" 0
}
check "uninstall plan comes from the manifest only, newest first, system packages are only listed" uninstall_from_manifest
resume_logic() {
  local st="$TMPD/state.tsv"
  printf 'core.git\tdone\tt\t\ncore.rustup\tfailed\tt\tboom\n' > "$st"
  step_skip resume "$st" core.git 0 || { echo "done step not skipped on resume"; return 1; }
  step_skip repair "$st" core.git 0 && { echo "repair must ignore the state file"; return 1; }
  step_skip resume "$st" core.rustup 0 && { echo "failed step must run again"; return 1; }
  step_skip resume "$st" core.rustup 1 || { echo "detected step must be skipped"; return 1; }
  step_skip normal "$TMPD/none" x 0 && return 1
  return 0
}
check "state and resume logic" resume_logic

# ---- dry-run: golden files and invariants ---------------------------------------------------------------
FAKE="$TMPD/fakehome"
dry() { # dry FAMILY COMPONENTS
  HOME="$FAKE" XDG_CONFIG_HOME="" XDG_DATA_HOME="" XDG_STATE_HOME="" CARGO_HOME="" CARGO_TARGET_DIR="" JAVA_HOME="" ANDROID_HOME="" ANDROID_SDK_ROOT="" ANDROID_NDK_HOME="" PISKA_ZIG="" \
    PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --components "$2" --assume-clean --assume-os "$1" --assume-arch x86_64 --prefix "$FAKE/prefix" 2>&1
}
normalize() { sed -e "s#$REPO#<REPO>#g" -e "s#$FAKE#<FAKE>#g" | sed -e "s#/c/Users/[^ \"]*/scratch-2026[^ \"/]*#<REPO>#g"; }
golden_case() { # golden_case FAMILY SET
  local out file="$GOLDEN/unix-$1-$2.txt"
  out=$(dry "$1" "$2"; echo "exit=$?")
  out=$(printf '%s\n' "$out" | normalize)
  [ -d "$FAKE" ] && { echo "dry run created $FAKE"; return 1; }
  case "$out" in *"exit=0") ;; *) echo "non-zero exit"; printf '%s\n' "$out" | tail -n 5; return 1 ;; esac
  if [ "$UPDATE" = 1 ]; then mkdir -p "$GOLDEN"; printf '%s\n' "$out" > "$file"; fi
  [ -f "$file" ] || { echo "no golden file $file (run with --update-golden)"; return 1; }
  [ "$(printf '%s\n' "$out")" = "$(cat "$file")" ] || { diff <(cat "$file") <(printf '%s\n' "$out") | head -n 8; return 1; }
}
for fam in debian fedora arch suse macos; do check "golden dry-run: $fam, all components" golden_case "$fam" all; done
for set in core cpp web android linux-cross engine; do check "golden dry-run: debian, $set" golden_case debian "$set"; done

all_subsets() {
  local mask i c list want out
  local comps="core cpp web android linux-cross engine"
  for mask in $(seq 1 63); do
    list=""; i=0
    for c in $comps; do [ $((mask >> i & 1)) = 1 ] && list="$list,$c"; i=$((i + 1)); done
    list=${list#,}
    out=$(dry debian "$list") || { echo "mask $mask ($list): non-zero exit"; return 1; }
    for c in $(printf '%s' "$list" | tr ',' ' '); do
      printf '%s\n' "$out" | grep -q "STEP $c\." || { echo "mask $mask ($list): no step for $c"; return 1; }
    done
    printf '%s\n' "$out" | grep -q 'STEP core\.' || { echo "mask $mask: core was not added"; return 1; }
    printf '%s\n' "$out" | grep -q 'SKIP ' && { echo "mask $mask: a clean machine must plan every step"; return 1; }
    printf '%s\n' "$out" | grep -q 'DRY RUN finished: no changes were made.' || { echo "mask $mask: no dry-run footer"; return 1; }
    [ -d "$FAKE" ] && { echo "mask $mask: dry run created $FAKE"; return 1; }
  done
  return 0
}
check "dry-run of every one of the 63 component combinations plans correctly and touches nothing" all_subsets

safety_facts() {
  local all; all=$(cat "$GOLDEN/unix-debian-all.txt")
  for needle in 'licence:' 'elevation:' 'verify   sha256' 'cross-checked with' 'run      sudo apt-get install' 'DRY RUN finished: no changes were made.'; do
    case "$all" in *"$needle"*) ;; *) echo "plan lacks '$needle'"; return 1 ;; esac
  done
  case "$all" in *"curl | sh"*|*"| sh"*|*"| bash"*) echo "plan pipes a download into a shell"; return 1 ;; esac
}
check "golden plans mention licence, elevation, checksums, the sudo command; nothing is piped into a shell" safety_facts
exit_codes() {
  local code
  PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --components nonsense --dry-run >/dev/null 2>&1; code=$?
  eq "$code" 2 || return 1
  PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --bogus >/dev/null 2>&1; code=$?
  eq "$code" 2
}
check "bad component / option exits with code 2" exit_codes
licence_gate() {
  YES=1; ACCEPT_LICENSES=0; confirm_license X https://example.com 2>/dev/null && return 1
  ACCEPT_LICENSES=1; confirm_license X https://example.com
}
check "a licence is never accepted by --yes alone" licence_gate

# ---- --json: the machine-readable stream of the graphical installer ----------------------------------------
PY=$(command -v python3 || command -v python || true)
cat > "$TMPD/render.py" <<'PYEOF'
# Rebuild the human dry-run text from --json events (proves both carry the same steps in the same order),
# or normalise a --json stream for its golden file. Usage: render.py render|normalize FILE [REPO FAKE]
import json, re, sys
mode, path = sys.argv[1], sys.argv[2]
raw = open(path, encoding="utf-8").read()
if mode == "normalize":
    for p in sys.argv[3:]:
        if p:
            raw = raw.replace(p, "<ROOT>")
    raw = re.sub(r'/c/Users/[^ "]*/scratch-2026[^ "/]*', "<ROOT>", raw)
    raw = re.sub(r'"shell":"[^"]*"', '"shell":"<SHELL>"', raw)
    raw = re.sub(r'"free_bytes":(\d+|null)', '"free_bytes":"<N>"', raw)
    raw = re.sub(r'"is_admin":(true|false)', '"is_admin":"<ADMIN>"', raw)
    raw = re.sub(r'"elevate":"[^"]*"', '"elevate":"<ELEVATE>"', raw)
    raw = re.sub(r'"path":"[^"]*","env":\{[^}]*\}', '"path":"<PATH>","env":"<ENV>"', raw)
    sys.stdout.write(raw.rstrip("\n") + "\n")
    sys.exit(0)
out = []
for line in raw.splitlines():
    if not line.strip():
        continue
    e = json.loads(line)
    if e["event"] == "log":
        out.append(e["text"])
    elif e["event"] == "plan":
        head = "[%d/%d] " % (e["n"], e["total"])
        if e["skip"]:
            why = {"requested": " (skipped on request)", "found": " (found: %s)" % e["found"]}.get(e["reason"], " (done earlier)")
            out.append(head + "SKIP %s - %s%s" % (e["id"], e["title"], why))
            continue
        out.append(head + "STEP %s - %s" % (e["id"], e["title"]))
        if e["elevation"]:
            out.append("         elevation: " + e["elevation"])
        if e["licence"]:
            out.append("         licence: %s (%s) - accepted only with --accept-licenses or your answer" % (e["licence"]["name"], e["licence"]["url"]))
        for l in e["lines"]:
            out.append("         " + l["kind"].ljust(8) + " " + l["text"])
sys.stdout.write("\n".join(out) + "\n")
PYEOF

json_escapes() {
  eq "$(json_str 'a"b\c')" '"a\"b\\c"' || return 1
  eq "$(json_str "$(printf 'x\ty')")" '"x\ty"' || return 1
  eq "$(json_str "$(printf 'l1\nl2')")" '"l1\nl2"' || return 1
  eq "$(printf '%s\n' a 'b c' | jarr)" '["a","b c"]' || return 1
  eq "$(printf 'run      cargo build\nnote     x  y\n' | plan_lines_json)" '[{"kind":"run","text":"cargo build"},{"kind":"note","text":"x  y"}]'
}
check "json_str / jarr / plan_lines_json produce valid JSON" json_escapes
json_answers() {
  local a
  for a in yes y YES true accept '{"answer":true}' ' yes '; do json_yes "$a" || { echo "'$a' must be a yes"; return 1; }; done
  for a in no n '' maybe '{"answer":false}'; do json_yes "$a" && { echo "'$a' is not a yes"; return 1; }; done
  # a prompt event goes to fd 3, the answer is read from fd 4; end of input is a no
  JSON=1; CURRENT_STEP=android.licenses
  exec 3> "$TMPD/prompt.ev" 4< <(printf 'yes\n')
  json_prompt licence "$(jkv licence android-sdk)" || { echo "yes on stdin not accepted"; return 1; }
  json_prompt confirm "$(jkv key k)" && { echo "end of input must be a no"; return 1; }
  exec 3>&- 4<&-
  JSON=0; CURRENT_STEP=""
  grep -q '^{"event":"prompt","kind":"licence","step":"android.licenses","licence":"android-sdk"}$' "$TMPD/prompt.ev" || { cat "$TMPD/prompt.ev"; return 1; }
}
check "prompt answers: only an explicit yes counts; prompt events carry the step" json_answers
licence_by_id() {
  YES=1; ACCEPT_LICENSES=0; ACCEPT_LICENSE=android-sdk
  confirm_license "the Android SDK" https://example.com android-sdk || { echo "named licence not accepted"; return 1; }
  confirm_license "Other" https://example.com other-id 2>/dev/null && { echo "another licence was accepted"; return 1; }
  ACCEPT_LICENSE=""; return 0
}
check "licences can be accepted one by one (--accept-license), never by --yes alone" licence_by_id
manual_sudo_command() {
  # --json without pkexec: nothing runs, the exact command for a terminal is reported instead
  JSON=1; ELEVATE=none; FAMILY=debian; STEP_COMPONENT=core
  exec 3> "$TMPD/manual.ev"
  pkg_install_run "libfoo-dev bar" 2> "$TMPD/manual.err" && { echo "the package step must not succeed without rights"; exec 3>&-; return 1; }
  exec 3>&-
  JSON=0
  eq "$MANUAL_COMMAND" "sudo apt-get update && sudo apt-get install -y libfoo-dev bar" || return 1
  grep -q 'run this command in a terminal' "$TMPD/manual.err"
}
if [ "$(id -u)" != 0 ]; then check "--json without pkexec: the package command is handed to the user, never run" manual_sudo_command; fi
download_progress() {
  JSON=1; CURRENT_STEP=linux-cross.zig
  curl_get() { printf '%04000d' 0 > "$2"; sleep 2; printf '%04000d' 0 >> "$2"; }
  exec 3> "$TMPD/progress.ev"
  curl_get_progress https://example.invalid/zig.tar.xz "$TMPD/zig.part" 1 || { exec 3>&-; unset -f curl_get; return 1; }
  exec 3>&-
  unset -f curl_get; JSON=0; CURRENT_STEP=""
  grep -q '^{"event":"progress","step":"linux-cross.zig","url":"https://example.invalid/zig.tar.xz","bytes":4000,"total":1048576,"estimated":true}$' "$TMPD/progress.ev" || { cat "$TMPD/progress.ev"; return 1; }
}
check "download progress events while curl runs (stubbed curl, no network)" download_progress
. "$INSTALL/install.sh"
set +e
DATA=$(toml_flatten "$INSTALL/packages.toml")

dry_json() { # dry_json FAMILY COMPONENTS [EXTRA...]
  local fam=$1 comps=$2; shift 2
  HOME="$FAKE" XDG_CONFIG_HOME="" XDG_DATA_HOME="" XDG_STATE_HOME="" CARGO_HOME="" CARGO_TARGET_DIR="" JAVA_HOME="" ANDROID_HOME="" ANDROID_SDK_ROOT="" ANDROID_NDK_HOME="" PISKA_ZIG="" \
    PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --components "$comps" --dry-run --assume-clean --json --assume-os "$fam" --assume-arch x86_64 --prefix "$FAKE/prefix" "$@" </dev/null
}
json_golden_case() { # json_golden_case FAMILY SET
  local out="$TMPD/j-$1-$2.jsonl" file="$GOLDEN/unix-json-$1-$2.jsonl" code
  dry_json "$1" "$2" > "$out" 2> "$TMPD/j.err"; code=$?
  [ "$code" = 0 ] || { echo "exit $code"; cat "$TMPD/j.err"; return 1; }
  [ -d "$FAKE" ] && { echo "dry run created $FAKE"; return 1; }
  grep -v '^{"event":"' "$out" | grep -q . && { echo "a line that is not an event:"; grep -v '^{"event":"' "$out" | head -n 3; return 1; }
  head -n 1 "$out" | grep -q '^{"event":"hello"' || { echo "first event is not hello"; return 1; }
  tail -n 1 "$out" | grep -q '^{"event":"summary","exit":0,' || { echo "last event is not a successful summary"; return 1; }
  [ -n "$PY" ] || { echo "(python not found: rendering and golden comparison skipped)"; return 0; }
  "$PY" "$TMPD/render.py" render "$out" | normalize > "$TMPD/rendered.txt" || return 1
  # the human golden ends with the exit line of golden_case
  head -n -1 "$GOLDEN/unix-$1-$2.txt" > "$TMPD/human.txt" 2>/dev/null || sed '$d' "$GOLDEN/unix-$1-$2.txt" > "$TMPD/human.txt"
  diff "$TMPD/human.txt" "$TMPD/rendered.txt" > "$TMPD/d.txt" || { echo "the --json plan differs from the human golden:"; head -n 8 "$TMPD/d.txt"; return 1; }
  # Paths are replaced by sed here: Git Bash would rewrite /tmp/... arguments of a native python.
  normalize < "$out" > "$TMPD/paths.jsonl"
  "$PY" "$TMPD/render.py" normalize "$TMPD/paths.jsonl" > "$TMPD/norm.jsonl" || return 1
  if [ "$UPDATE" = 1 ]; then cp "$TMPD/norm.jsonl" "$file"; fi
  [ -f "$file" ] || { echo "no golden file $file (run with --update-golden)"; return 1; }
  diff "$file" "$TMPD/norm.jsonl" > "$TMPD/d.txt" || { cut -c1-200 "$TMPD/d.txt" | head -n 6; return 1; }
}
for fam in debian macos; do check "golden --json dry-run: $fam, all components (matches the human golden)" json_golden_case "$fam" all; done
for set in core cpp web android linux-cross engine; do check "golden --json dry-run: debian, $set (matches the human golden)" json_golden_case debian "$set"; done

json_all_subsets() {
  local mask i c list human js
  local comps="core cpp web android linux-cross engine"
  for mask in $(seq 1 63); do
    list=""; i=0
    for c in $comps; do [ $((mask >> i & 1)) = 1 ] && list="$list,$c"; i=$((i + 1)); done
    list=${list#,}
    human=$(dry debian "$list" | sed -n 's/^\[[0-9]*\/[0-9]*\] STEP \([^ ]*\) .*/\1/p' | tr '\n' ' ')
    js=$(dry_json debian "$list" 2>/dev/null | sed -n 's/^{"event":"plan","n":[0-9]*,"total":[0-9]*,"id":"\([^"]*\)".*/\1/p' | tr '\n' ' ')
    [ -n "$js" ] && [ "$human" = "$js" ] || { echo "mask $mask ($list): human '$human' vs json '$js'"; return 1; }
    [ -d "$FAKE" ] && { echo "mask $mask: dry run created $FAKE"; return 1; }
  done
  return 0
}
check "--json dry-run of all 63 combinations: the same steps in the same order as the human plan" json_all_subsets

json_errors() {
  local out code
  out=$(PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --components nonsense --dry-run --json --assume-os debian </dev/null 2>/dev/null); code=$?
  eq "$code" 2 || return 1
  printf '%s\n' "$out" | head -n 1 | grep -q '^{"event":"error","text":"unknown component' || { printf '%s\n' "$out"; return 1; }
  printf '%s\n' "$out" | tail -n 1 | grep -q '^{"event":"summary","exit":2,' || { printf '%s\n' "$out"; return 1; }
  out=$(dry_json debian core --source "$TMPD" 2>/dev/null); code=$?
  eq "$code" 2 || return 1
  printf '%s\n' "$out" | grep -q 'not an engine checkout' || return 1
  dry_json debian core --elevate bogus >/dev/null 2>&1; eq "$?" 2
}
check "--json: bad arguments end with an error event, a summary and exit code 2" json_errors
json_skip_and_source() {
  local out
  out=$(dry_json debian core --skip-steps core.vulkan,core.rustup 2>/dev/null)
  printf '%s\n' "$out" | grep -q '"id":"core.vulkan","component":"core","title":"[^"]*","skip":true,"reason":"requested"' || { echo "core.vulkan was not skipped on request"; return 1; }
  printf '%s\n' "$out" | grep -q '"id":"core.rustup","component":"core","title":"[^"]*","skip":false' || { echo "core.rustup must not be skippable"; return 1; }
  printf '%s\n' "$out" | grep -q '"level":"warn".*core.rustup cannot be skipped' || { echo "no warning for core.rustup"; return 1; }
  out=$(dry_json debian engine --source "$REPO" 2>/dev/null)
  printf '%s\n' "$out" | head -n 1 | grep -q '"in_checkout":true' || { echo "--source not used"; return 1; }
}
check "--json: --skip-steps skips only skippable steps; --source uses an existing checkout" json_skip_and_source
json_uninstall() {
  local st="$FAKE/state/PiskaEngine/install" out
  mkdir -p "$st"
  printf 'rust-target\twasm32-unknown-unknown\t\t\tweb\tt\nfile\tpiska-hub.desktop\t%s\t\tengine\tt\ndir\tzig\t/x/zig\t\tlinux-cross\tt\n' "$TMPD/victim.desktop" > "$st/manifest.tsv"
  : > "$TMPD/victim.desktop"
  out=$(HOME="$FAKE" XDG_STATE_HOME="$FAKE/state" PISKA_INSTALL_NOMAIN="" bash "$INSTALL/install.sh" --uninstall --dry-run --json --assume-os debian </dev/null 2>/dev/null)
  rm -rf "$FAKE"
  printf '%s\n' "$out" | grep -q '^{"event":"uninstall-plan",.*"items":\[{"action":"delete-dir","target":"/x/zig","ask":true},{"action":"delete-file","target":"[^"]*victim.desktop","ask":false},{"action":"rustup-target-remove","target":"wasm32-unknown-unknown","ask":false}\]}$' || { printf '%s\n' "$out"; return 1; }
  [ -f "$TMPD/victim.desktop" ] || { echo "dry run deleted a file"; return 1; }
  printf '%s\n' "$out" | tail -n 1 | grep -q '^{"event":"summary","exit":0,'
}
check "uninstall --json --dry-run: the manifest as an uninstall-plan event, nothing removed" json_uninstall

echo
echo "$PASSED passed, $FAILED failed"
[ "$FAILED" = 0 ]
