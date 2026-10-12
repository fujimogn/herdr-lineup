#!/usr/bin/env bash
set -euo pipefail

clean_env() {
    local key
    local -a unset_args=()
    while IFS= read -r key; do
        [[ $key == HERDR_* ]] && unset_args+=(-u "$key")
    done < <(compgen -e)
    env -u ENV -u BASH_ENV "${unset_args[@]}" "$@"
}

guard_socket() {
    python3 - "$@" <<'PY'
import os, sys
raw_expected = os.path.abspath(sys.argv[2])
candidate, expected, *forbidden = map(os.path.realpath, sys.argv[1:])
def inside(path, root):
    return path == root or path.startswith(root + os.sep)
if expected != raw_expected or not inside(candidate, expected) or any(inside(candidate, p) for p in forbidden):
    sys.exit('Refusing an endpoint outside the isolated session (including main).')
PY
}

# These tests never start Herdr or connect to a socket.
self_test() {
    local root bad
    root=$(mktemp -d /tmp/lineup-guard.XXXXXXXX)
    root=$(cd "$root" && pwd -P)
    mkdir -p "$root/sessions/lineup-e2e-test" "$root/sessions/main"
    bad="$root/sessions/main/api.sock"
    if guard_socket "$bad" "$root/sessions/lineup-e2e-test" "$root/sessions/main"; then
        echo 'FAIL: main socket accepted' >&2; return 1
    fi
    ln -s "$root/sessions/main" "$root/sessions/lineup-e2e-test/redirect"
    if guard_socket "$root/sessions/lineup-e2e-test/redirect/api.sock" "$root/sessions/lineup-e2e-test" "$root/sessions/main"; then
        echo 'FAIL: symlink to main accepted' >&2; return 1
    fi
    guard_socket "$root/sessions/lineup-e2e-test/api.sock" "$root/sessions/lineup-e2e-test" "$root/sessions/main"
    export HERDR_SOCKET_PATH="$bad" HERDR_SESSION=main HERDR_UNKNOWN_TEST=poison
    # shellcheck disable=SC2016
    clean_env bash -c '[[ -z ${HERDR_SOCKET_PATH+x}${HERDR_SESSION+x}${HERDR_UNKNOWN_TEST+x} ]]'
    unlink "$root/sessions/lineup-e2e-test/redirect"
    rmdir "$root/sessions/lineup-e2e-test" "$root/sessions/main" "$root/sessions" "$root"
    echo 'PASS: main rejection, symlink rejection, safe path, all HERDR_* cleared'
}

if [[ ${1:-} == --self-test ]]; then
    self_test
    exit
fi

tokens=false
case ${1:-} in
    '') ;;
    --tokens) tokens=true ;;
    *) echo 'Usage: e2e-isolated.sh [--self-test|--tokens]' >&2; exit 2 ;;
esac
repo=$(cd "$(dirname "$0")/.." && pwd -P)
herdr_bin=$(command -v herdr)
real_home=${HOME:?}
real_config=${XDG_CONFIG_HOME:-$real_home/.config}
session="lineup-e2e-$(date -u +%Y%m%dT%H%M%S)-$$"
[[ $session == lineup-e2e-* && $session != main ]] || exit 1
cargo build --release --locked --manifest-path "$repo/Cargo.toml"
binary="$repo/target/release/herdr-lineup"
[[ -x $binary ]] || { echo 'Expected release binary is missing' >&2; exit 1; }
# Short paths are necessary for Unix socket length limits on macOS and Linux.
temp=$(mktemp -d /tmp/lineup-e2e.XXXXXXXX)
temp=$(cd "$temp" && pwd -P)
chmod 700 "$temp"
home="$temp/home"
config="$home/.config"
session_dir="$config/herdr/sessions/$session"
socket_path="$session_dir/not-yet-created.sock"
server_started=false
daemon_started=false
server_pid=''
isolated_env() {
    clean_env env HOME="$home" XDG_CONFIG_HOME="$config" XDG_DATA_HOME="$home/.local/share" \
        XDG_STATE_HOME="$home/.local/state" XDG_CACHE_HOME="$home/.cache" \
        CODEX_HOME="$home/.codex" GROK_HOME="$home/.grok" \
        SHELL=/bin/sh HERDR_CONFIG_PATH="$config/herdr/config.toml" "$@"
}
check_endpoint() {
    guard_socket "$socket_path" "$session_dir" "$real_config/herdr/sessions/main" \
        "$real_home/.config/herdr/sessions/main" "$config/herdr/sessions/main"
}
herdr_cmd() {
    check_endpoint
    isolated_env "$herdr_bin" --session "$session" "$@"
}
lineup_cmd() {
    check_endpoint
    [[ -S $socket_path ]] || { echo 'Isolated socket missing' >&2; return 1; }
    isolated_env env HERDR_SOCKET_PATH="$socket_path" HERDR_SESSION="$session" \
        HERDR_PLUGIN_STATE_DIR="$temp/state" HERDR_PLUGIN_CONFIG_DIR="$temp/settings" \
        "$binary" "$@"
}
cleanup() {
    local result=$? failed=false
    trap - EXIT INT TERM
    # If a safety check fails, do not send even cleanup operations to that socket.
    if ! check_endpoint; then
        echo "Cleanup refused; retained private directory: $temp" >&2
        exit 1
    fi
    if $daemon_started; then lineup_cmd stop || failed=true; fi
    if $server_started; then
        if herdr_cmd session stop "$session"; then
            for ((attempt=0; attempt<100; attempt++)); do
                kill -0 "$server_pid" 2>/dev/null || break
                sleep 0.1
            done
            if kill -0 "$server_pid" 2>/dev/null; then
                failed=true
            else
                wait "$server_pid" || true
                herdr_cmd session delete "$session" || failed=true
            fi
        else
            failed=true
        fi
        [[ ! -e $session_dir ]] || failed=true
    fi
    if $failed; then
        echo "Cleanup incomplete; retained private directory: $temp" >&2
        exit 1
    fi
    # Only remove the unique mktemp directory owned by this invocation.
    python3 - "$temp" <<'PY'
import os, shutil, sys
path = sys.argv[1]
if not os.path.basename(path).startswith('lineup-e2e.') or os.path.dirname(path) != os.path.realpath('/tmp') or os.path.islink(path) or os.stat(path).st_uid != os.getuid():
    sys.exit('Refusing temporary directory cleanup')
shutil.rmtree(path)
PY
    [[ ! -e $temp ]] || exit 1
    echo 'PASS: isolated session and temporary directory removed'
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$config/herdr" "$temp/state" "$temp/settings" "$temp/workspace"
cat > "$config/herdr/config.toml" <<'TOML'
[terminal]
default_shell = "/bin/sh"
shell_mode = "non_login"
[update]
version_check = false
manifest_check = false
TOML
(cd "$temp/workspace" && herdr_cmd server) >"$temp/server.log" 2>&1 &
server_pid=$!
server_started=true
# Discover the socket from the private session directory, never inherited env.
for ((attempt=0; attempt<100; attempt++)); do
    discovered_socket=$(python3 - "$session_dir" <<'PY'
import os, stat, sys
root = sys.argv[1]
# Herdr also has herdr-client.sock; only herdr.sock is the JSON API endpoint.
sockets = [os.path.join(root, name) for name in os.listdir(root)
           if name == 'herdr.sock' and stat.S_ISSOCK(os.lstat(os.path.join(root, name)).st_mode)] if os.path.isdir(root) else []
if len(sockets) == 1:
    print(sockets[0])
PY
)
    if [[ -n $discovered_socket ]]; then socket_path=$discovered_socket; break; fi
    kill -0 "$server_pid" 2>/dev/null || { echo 'Isolated server exited' >&2; exit 1; }
    sleep 0.1
done
check_endpoint
[[ -S $socket_path ]] || { echo 'Isolated server socket timed out' >&2; exit 1; }
herdr_cmd workspace create --cwd "$temp/workspace" --label lineup-e2e --no-focus > "$temp/workspace.json"
pane=$(python3 - "$temp/workspace.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))['result']['root_pane']['pane_id'])
PY
)
[[ -n $pane ]] || exit 1
echo 'PASS: private workspace and root pane created'
daemon_started=true
lineup_cmd ensure
pid_record() {
    python3 - "$temp/state" <<'PY'
import glob, sys
paths = glob.glob(sys.argv[1] + '/session-*/daemon.pid')
if len(paths) != 1:
    sys.exit('Expected exactly one daemon PID record')
print(open(paths[0]).read().strip())
PY
}
first_pid=$(pid_record)
lineup_cmd ensure &
ensure_pid=$!
lineup_cmd ensure
wait "$ensure_pid"
[[ $(pid_record) == "$first_pid" ]] || { echo 'Daemon PID changed on repeated ensure' >&2; exit 1; }
lineup_cmd doctor
echo 'PASS: ensure, concurrent ensure single PID, doctor'
if $tokens; then
    # Explicit synthetic occupant: no real agent process or provider session.
    herdr_cmd pane report-agent "$pane" --source lineup-e2e --agent codex --state idle > "$temp/agent.json"
    found=false
    deadline=$((SECONDS + 10))
    while ((SECONDS < deadline)); do
        herdr_cmd api snapshot > "$temp/snapshot.json"
        if python3 - "$temp/snapshot.json" <<'PY'
import json, sys
def has_token(value):
    if isinstance(value, dict):
        return any(k.startswith('lineup_') or has_token(v) for k, v in value.items())
    if isinstance(value, list):
        return any(map(has_token, value))
    return False
sys.exit(0 if has_token(json.load(open(sys.argv[1]))) else 1)
PY
        then found=true; break; fi
        sleep 0.1
    done
    $found || { echo 'No lineup_ metadata in isolated snapshot' >&2; exit 1; }
    echo 'PASS: lineup_ metadata received (snapshot contents not printed)'
else
    echo 'SKIP: publication tokens; rerun with --tokens after publication wiring lands'
fi
lineup_cmd stop
lineup_cmd stop
daemon_started=false
if pid_record > /dev/null 2>&1; then echo 'Daemon PID record remained after stop' >&2; exit 1; fi
echo 'PASS: stop and idempotent stop'
