#!/usr/bin/env bash
set -euo pipefail

clean_env() {
    local key
    # A nonempty command array also works with macOS Bash 3.2 and nounset.
    local -a env_command=(env -u ENV -u BASH_ENV)
    while IFS= read -r key; do
        [[ $key == HERDR_* ]] && env_command+=(-u "$key")
    done < <(compgen -e)
    "${env_command[@]}" "$@"
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

# Validate only display tokens; never print snapshot paths or session contents.
token_check() {
    python3 - "$@" <<'PYTOK'
import copy, datetime, json, sys

def require(value):
    if not value: raise ValueError('token mismatch')

def check(snapshot, workspace, pane, ship):
    rows = snapshot['result']['snapshot']
    w = next(r for r in rows['workspaces'] if r['workspace_id'] == workspace)
    tokens = w.get('tokens', {})
    require(tokens.get('lineup_repo') == 'lineup-e2e')
    require(tokens.get('lineup_ship_unix') == ship)
    stamp = datetime.datetime.fromtimestamp(int(ship))
    expected_text = stamp.strftime('%H:%M') if stamp.date() == datetime.datetime.now().date() else f'{stamp.month}/{stamp.day}'
    require(tokens.get('lineup_ship_text') == expected_text)
    if pane:
        p = next(r for r in rows['panes'] if r['pane_id'] == pane)
        tokens = p.get('tokens', {})
        expected = {'lineup_agent': 'codex', 'lineup_logo': 'Codex',
                    'lineup_state': 'idle', 'lineup_state_idle': '●',
                    'lineup_context_text': '—', 'lineup_weekly_text': '—'}
        require(all(tokens.get(k) == v for k, v in expected.items()))
        require('lineup_context_pct' not in tokens and 'lineup_weekly_pct' not in tokens)

def cleared(snapshot):
    rows = snapshot['result']['snapshot']
    require(all(not any(k.startswith('lineup_') for k in row.get('tokens', {}))
               for kind in ('panes', 'workspaces') for row in rows[kind]))

if sys.argv[1] == 'self-test':
    fixture = {'result': {'snapshot': {'workspaces': [{
        'workspace_id': 'w1', 'tokens': {'lineup_repo': 'lineup-e2e',
        'lineup_ship_unix': '123', 'lineup_ship_text': '1/1'}}], 'panes': [{
        'pane_id': 'w1:p1', 'tokens': {'lineup_agent': 'codex',
        'lineup_logo': 'Codex', 'lineup_state': 'idle', 'lineup_state_idle': '●',
        'lineup_context_text': '—', 'lineup_weekly_text': '—'}}]}}}
    stamp = datetime.datetime.fromtimestamp(123)
    fixture['result']['snapshot']['workspaces'][0]['tokens']['lineup_ship_text'] = stamp.strftime('%H:%M') if stamp.date() == datetime.datetime.now().date() else f'{stamp.month}/{stamp.day}'
    check(fixture, 'w1', 'w1:p1', '123')
    for kind, key, value in [('workspaces', 'lineup_repo', '—'),
                             ('workspaces', 'lineup_ship_unix', '124'),
                             ('panes', 'lineup_weekly_text', '7d 99%'),
                             ('panes', 'lineup_context_pct', '12')]:
        bad = copy.deepcopy(fixture)
        bad['result']['snapshot'][kind][0]['tokens'][key] = value
        try: check(bad, 'w1', 'w1:p1', '123')
        except ValueError: pass
        else: sys.exit('FAIL: incorrect tokens accepted')
    for kind, key in [('workspaces', 'lineup_ship_unix'), ('panes', 'lineup_state')]:
        bad = copy.deepcopy(fixture)
        del bad['result']['snapshot'][kind][0]['tokens'][key]
        try: check(bad, 'w1', 'w1:p1', '123')
        except ValueError: pass
        else: sys.exit('FAIL: missing token accepted')
    try: cleared(fixture)
    except ValueError: pass
    else: sys.exit('FAIL: uncleared tokens accepted')
    for kind in ('workspaces', 'panes'):
        fixture['result']['snapshot'][kind][0]['tokens'] = {'other_plugin': 'kept'}
    cleared(fixture)
    print('PASS: token validation rejects wrong/missing values and uncleared tokens')
else:
    snapshot = json.load(open(sys.argv[2]))
    try:
        if sys.argv[1] == 'clear': cleared(snapshot)
        else: check(snapshot, *sys.argv[3:])
    except (ValueError, KeyError, StopIteration):
        sys.exit(1)
PYTOK
}

sequence_record() {
    python3 - "$temp/state" "$workspace" "$pane" <<'PYSEQ'
import glob, json, sys
paths = glob.glob(sys.argv[1] + '/session-*/sequences.json')
if len(paths) != 1: sys.exit('Expected one isolated sequence checkpoint')
rows = json.load(open(paths[0]))
result = {}
for kind, target in [('workspace', sys.argv[2]), ('pane', sys.argv[3])]:
    matches = [r['seq'] for r in rows if r['kind'] == kind and r['id'] == target]
    if len(matches) != 1: sys.exit('Expected one checkpoint per tested resource')
    result[kind] = matches[0]
print(json.dumps(result, sort_keys=True))
PYSEQ
}

sequence_check() {
    python3 - "$@" <<'PYSEQCHECK'
import json, sys

def advanced(first, second):
    return all(type(first.get(k)) is int and type(second.get(k)) is int
               and second[k] > first[k] >= 1 for k in ('workspace', 'pane'))
if sys.argv[1] == 'self-test':
    first = {'workspace': 1, 'pane': 2}
    if not advanced(first, {'workspace': 2, 'pane': 3}): sys.exit('FAIL: renewal rejected')
    for bad in [first, {'workspace': 2, 'pane': 2}, {'workspace': 2},
                {'workspace': True, 'pane': 3}]:
        if advanced(first, bad): sys.exit('FAIL: invalid renewal accepted')
    print('PASS: renewal validation rejects unchanged or missing sequences')
else:
    first, second = [json.load(open(p)) for p in sys.argv[1:]]
    if not advanced(first, second): sys.exit('Sequences did not advance for both resources')
    print('Sequence renewal: ' + json.dumps(first, sort_keys=True) + ' -> ' + json.dumps(second, sort_keys=True))
PYSEQCHECK
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
    token_check self-test
    sequence_check self-test
    echo 'PASS: main rejection, symlink rejection, safe path, all HERDR_* cleared'
}

if [[ ${1:-} == --self-test ]]; then
    self_test
    exit
fi

tokens=true
case ${1:-} in
    '') ;;
    --tokens) tokens=true ;;
    *) echo 'Usage: e2e-isolated.sh [--self-test|--tokens]' >&2; exit 2 ;;
esac
repo=$(cd "$(dirname "$0")/.." && pwd -P)
herdr_bin=$(command -v herdr)
real_home=${HOME:?}
real_config=${XDG_CONFIG_HOME:-$real_home/.config}
session="lineup-e2e-$(date -u +%H%M%S)-$$"
[[ $session == lineup-e2e-* && $session != main ]] || exit 1
cargo build --release --locked --manifest-path "$repo/Cargo.toml"
binary="$repo/target/release/herdr-lineup"
[[ -x $binary ]] || { echo 'Expected release binary is missing' >&2; exit 1; }
# Short paths are necessary for Unix socket length limits on macOS and Linux.
temp=$(mktemp -d /tmp/lineup-e2e.XXXXXXXX)
temp=$(cd "$temp" && pwd -P)
chmod 700 "$temp"
home="$temp/h"
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
mkdir -p "$config/herdr" "$temp/state" "$temp/settings"
# A private local clone gives the workspace a Git identity and origin/main.
# No network, user configuration, hooks, or authenticated provider files copied.
clean_env env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git clone --quiet --no-local "$repo" "$temp/lineup-e2e"
ship_commit=$(git -C "$repo" rev-parse refs/remotes/origin/main)
ship_unix=$(git -C "$repo" show -s --format=%ct "$ship_commit")
clean_env env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$temp/lineup-e2e" update-ref refs/remotes/origin/main "$ship_commit"
cat > "$config/herdr/config.toml" <<'TOML'
[terminal]
default_shell = "/bin/sh"
shell_mode = "non_login"
[update]
version_check = false
manifest_check = false
TOML
(cd "$temp/lineup-e2e" && herdr_cmd server) >"$temp/server.log" 2>&1 &
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
herdr_cmd workspace create --cwd "$temp/lineup-e2e" --label lineup-e2e --no-focus > "$temp/workspace.json"
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
echo 'PASS: ensure, concurrent ensure single PID'
if $tokens; then
    # Explicit synthetic occupant: no real agent process or provider session.
    herdr_cmd pane report-agent "$pane" --source lineup-e2e --agent codex --state idle > "$temp/agent.json"
    workspace=$(python3 - "$temp/workspace.json" <<'PYID'
import json,sys
r=json.load(open(sys.argv[1]))['result']
print(r['workspace']['workspace_id'])
PYID
)
    found=false
    deadline=$((SECONDS + 12))
    while ((SECONDS < deadline)); do
        herdr_cmd api snapshot > "$temp/snapshot.json"
        if token_check present "$temp/snapshot.json" "$workspace" "$pane" "$ship_unix"; then
            found=true; break
        fi
        sleep 0.2
    done
    $found || { echo 'Expected workspace/pane lineup_ tokens missing or incorrect' >&2; exit 1; }
    python3 - "$temp/snapshot.json" "$workspace" "$pane" <<'PYVIEW'
import json, sys
rows=json.load(open(sys.argv[1]))['result']['snapshot']
for kind, id_key, target in [('workspaces','workspace_id',sys.argv[2]),('panes','pane_id',sys.argv[3])]:
    row=next(r for r in rows[kind] if r[id_key]==target)
    print(kind + ': ' + json.dumps({k:v for k,v in row.get('tokens',{}).items() if k.startswith('lineup_')}, ensure_ascii=False, sort_keys=True))
PYVIEW
    echo 'PASS: workspace heading and synthetic pane display tokens received'
    echo 'SKIP: real agent pane; isolated HOME has no provider authentication; synthetic occupant only'
    sequence_record > "$temp/seq-first.json"
    # The default TTL is 15 seconds; unchanged keys renew by 10 seconds.
    # A second snapshot after 16 seconds plus a newer checkpoint proves renewal.
    sleep 16
    herdr_cmd api snapshot > "$temp/renewed.json"
    token_check present "$temp/renewed.json" "$workspace" "$pane" "$ship_unix" || { echo 'Tokens expired before renewal' >&2; exit 1; }
    sequence_record > "$temp/seq-second.json"
    sequence_check "$temp/seq-first.json" "$temp/seq-second.json"
    echo 'PASS: second snapshot after default TTL retains tokens with newer sequences'
else
    echo 'SKIP: publication tokens; rerun with --tokens after publication wiring lands'
fi
lineup_cmd doctor
lineup_cmd stop
lineup_cmd stop
daemon_started=false
if pid_record > /dev/null 2>&1; then echo 'Daemon PID record remained after stop' >&2; exit 1; fi
echo 'PASS: stop and idempotent stop'
if $tokens; then
    herdr_cmd api snapshot > "$temp/stopped.json"
    token_check clear "$temp/stopped.json" || { echo 'lineup_ tokens remained after stop' >&2; exit 1; }
    echo 'PASS: all lineup_ tokens removed after stop'
fi
