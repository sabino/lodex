#!/bin/sh
# Native CLI gate. Writes only to a newly created, explicitly supplied test home.
set -eu
binary=${1:?usage: qualify-cli.sh BINARY NEW_TEST_HOME}
target_home=${2:?supply a new test home}
[ -x "$binary" ] || exit 2
[ ! -e "$target_home" ] || { echo 'test home must not already exist' >&2; exit 2; }
umask 077
mkdir -p "$target_home/tmp"
export CODEX_HOME="$target_home"
export TMPDIR="$target_home/tmp"
unset OPENAI_API_KEY CODEX_API_KEY CODEX_ACCESS_TOKEN
cat > "$target_home/config.toml" <<'EOF'
cli_auth_credentials_store = "file"
check_for_update_on_startup = false
[features]
apps = false
plugins = false
skill_search = false
code_mode_host = false
EOF
pass() { printf 'PASS %s\n' "$1"; }

"$binary" --version > "$target_home/version.txt"
grep -q '^lodex 0\.166\.0$' "$target_home/version.txt"
pass version
"$binary" --help > "$target_home/help.txt"
for command in app-server remote-control mcp exec exec-server login logout; do
    grep -q "$command" "$target_home/help.txt"
done
pass root-command-schema
"$binary" exec-server --listen ws://127.0.0.1:0 --concurrent-requests 2 --ws-auth capability-token --ws-token-file /tmp/lodex-executor-fixture.token --help > "$target_home/executor-help.txt"
grep -q -- '--concurrent-requests' "$target_home/executor-help.txt"
grep -q -- '--ws-auth' "$target_home/executor-help.txt"
pass remote-executor-schema
"$binary" app-server --managed-daemon --remote-control --listen unix:///tmp/lodex-fixture.sock --analytics-default-enabled -c features.apps=false --help > "$target_home/daemon-help.txt"
grep -q -- '--managed-daemon' "$target_home/daemon-help.txt"
pass managed-daemon-reexec-schema
"$binary" app-server daemon pid-update-loop --check-package-ownership
pass daemon-package-probe
"$binary" remote-control pair --help > "$target_home/pair-help.txt"
grep -q -- '--json' "$target_home/pair-help.txt"
pass remote-pair-schema

set +e
"$binary" app-server --stdio --listen off > "$target_home/conflict.txt" 2>&1
conflict_status=$?
set -e
[ "$conflict_status" -eq 2 ]
pass conflicting-transports-rejected

"$binary" mcp add fixture -- /bin/echo lodex-fixture > "$target_home/mcp-add.txt"
"$binary" mcp get fixture --json > "$target_home/mcp-get.json"
grep -q '"command": "/bin/echo"' "$target_home/mcp-get.json"
grep -q 'lodex-fixture' "$target_home/mcp-get.json"
"$binary" mcp remove fixture > "$target_home/mcp-remove.txt"
"$binary" mcp list --json > "$target_home/mcp-list.json"
grep -q '^\[\]$' "$target_home/mcp-list.json"
pass mcp-stdio-config-roundtrip

"$binary" mcp add fixture-http --url http://127.0.0.1:1/mcp > "$target_home/mcp-http-add.txt"
"$binary" mcp get fixture-http --json > "$target_home/mcp-http-get.json"
grep -q 'http://127.0.0.1:1/mcp' "$target_home/mcp-http-get.json"
"$binary" mcp remove fixture-http > "$target_home/mcp-http-remove.txt"
pass mcp-http-config-roundtrip

set +e
"$binary" login status > "$target_home/login-status.txt" 2>&1
login_status=$?
set -e
[ "$login_status" -eq 1 ]
grep -q 'Not logged in' "$target_home/login-status.txt"
pass isolated-login-status
