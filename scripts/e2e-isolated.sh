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

# pane_tokens in src/tokens.rs publishes this for missing context.
MISSING_CONTEXT_TEXT = '\u2014\u2800\u2800'

def code_points(value):
    if value is None:
        return 'missing'
    if not isinstance(value, str):
        return 'non-string'
    if value == '':
        return 'empty'
    return ' '.join(f'U+{ord(ch):04X}' for ch in value)

def workspace_expected(ship):
    stamp = datetime.datetime.fromtimestamp(int(ship))
    text = stamp.strftime('%H:%M') if stamp.date() == datetime.datetime.now().date() else f'{stamp.month}/{stamp.day}'
    return {'lineup_repo': 'lineup-e2e', 'lineup_ship_unix': str(ship), 'lineup_ship_text': text}

def pane_expected():
    return {'lineup_agent': 'codex', 'lineup_logo': 'Codex', 'lineup_state': 'idle',
            'lineup_state_idle': '●', 'lineup_context_text': MISSING_CONTEXT_TEXT,
            'lineup_weekly_text': '—'}

def diff_map(tokens, expected, absent):
    if not isinstance(tokens, dict):
        return ['tokens expected object actual missing']
    lines = []
    for key, want in expected.items():
        if key not in tokens or tokens[key] != want:
            got = tokens[key] if key in tokens else None
            lines.append(f'{key} expected {code_points(want)} actual {code_points(got)}')
    for key in absent:
        if key in tokens:
            lines.append(f'{key} expected absent actual {code_points(tokens[key])}')
    return lines

def diff_snapshot(snapshot, workspace, pane, ship):
    try:
        rows = snapshot['result']['snapshot']
        work = next(r for r in rows['workspaces'] if r['workspace_id'] == workspace)
        lines = diff_map(work.get('tokens') or {}, workspace_expected(ship), ())
        if pane:
            row = next(r for r in rows['panes'] if r['pane_id'] == pane)
            lines.extend(diff_map(row.get('tokens') or {}, pane_expected(),
                                  ('lineup_context_pct', 'lineup_weekly_pct')))
        return lines
    except (KeyError, StopIteration, TypeError, ValueError):
        return ['snapshot expected workspace and pane token rows actual missing']

def check(snapshot, workspace, pane, ship):
    if diff_snapshot(snapshot, workspace, pane, ship):
        raise ValueError('token mismatch')

def require(value):
    if not value:
        raise ValueError('token mismatch')

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
        'lineup_context_text': MISSING_CONTEXT_TEXT, 'lineup_weekly_text': '—'}}]}}}
    stamp = datetime.datetime.fromtimestamp(123)
    fixture['result']['snapshot']['workspaces'][0]['tokens']['lineup_ship_text'] = stamp.strftime('%H:%M') if stamp.date() == datetime.datetime.now().date() else f'{stamp.month}/{stamp.day}'
    good = copy.deepcopy(fixture)
    check(fixture, 'w1', 'w1:p1', '123')
    for kind, key, value in [('workspaces', 'lineup_repo', '—'),
                             ('workspaces', 'lineup_ship_unix', '124'),
                             ('panes', 'lineup_weekly_text', '7d 99%'),
                             ('panes', 'lineup_context_text', '—'),
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
    bare = copy.deepcopy(good)
    bare['result']['snapshot']['panes'][0]['tokens']['lineup_context_text'] = '—'
    reported = diff_snapshot(bare, 'w1', 'w1:p1', '123')
    if reported != ['lineup_context_text expected U+2014 U+2800 U+2800 actual U+2014']:
        sys.exit('FAIL: mismatch report')
    extra = copy.deepcopy(good)
    extra['result']['snapshot']['panes'][0]['tokens']['lineup_group'] = 'lineup-e2e'
    if diff_snapshot(extra, 'w1', 'w1:p1', '123'):
        sys.exit('FAIL: extra key reported')
    print('PASS: token validation rejects wrong/missing values and uncleared tokens')
else:
    snapshot = json.load(open(sys.argv[2]))
    try:
        if sys.argv[1] == 'clear': cleared(snapshot)
        elif sys.argv[1] == 'explain':
            lines = diff_snapshot(snapshot, *sys.argv[3:])
            if lines:
                print('\n'.join(lines), file=sys.stderr)
                sys.exit(1)
        else: check(snapshot, *sys.argv[3:])
    except (ValueError, KeyError, StopIteration):
        sys.exit(1)
PYTOK
}

# The publisher writes sequences.json only when it reserves a sequence window or
# prunes state, so renewals inside one window legitimately leave it unchanged.
sequence_tool() {
    python3 - "$@" <<'PYSEQ'
import glob, json, sys

def is_seq(value):
    return type(value) is int and value >= 1

def summarize(rows, workspace, pane):
    """Endpoint floor (exactly one) plus optional tested resources, each <= floor."""
    if not isinstance(rows, list) or not all(isinstance(r, dict) for r in rows):
        raise ValueError('checkpoint is not a list of rows')
    for r in rows:
        if not all(isinstance(r.get(k), str) for k in ('endpoint', 'kind', 'id')) or not is_seq(r.get('seq')):
            raise ValueError('checkpoint row has an invalid type')
    floors = [r for r in rows if r['kind'] == 'endpoint']
    if len(floors) != 1 or floors[0]['id'] != '':
        raise ValueError('expected exactly one endpoint floor row with an empty id')
    result = {'endpoint': floors[0]['seq']}
    for kind, target in [('workspace', workspace), ('pane', pane)]:
        matches = [r['seq'] for r in rows if r['kind'] == kind and r['id'] == target]
        if len(matches) > 1:
            raise ValueError('duplicate checkpoint row for a tested resource')
        if matches:
            if matches[0] > result['endpoint']:
                raise ValueError('resource sequence exceeds the endpoint floor')
            result[kind] = matches[0]
    return result

def non_decreasing(first, second):
    """Equal values are legal; a lower value or a vanished resource is not."""
    return all(k in second and second[k] >= first[k] for k in first)

def rejected(fn, *args):
    try: fn(*args)
    except ValueError: return True
    return False

if sys.argv[1] == 'self-test':
    floor = {'endpoint': 'ep', 'kind': 'endpoint', 'id': '', 'seq': 32}
    pane = {'endpoint': 'ep', 'kind': 'pane', 'id': 'w1:p1', 'seq': 30}
    # The first reservation writes only the endpoint floor row.
    if summarize([floor], 'w1', 'w1:p1') != {'endpoint': 32}: sys.exit('FAIL: endpoint-only checkpoint rejected')
    if summarize([floor, pane], 'w1', 'w1:p1') != {'endpoint': 32, 'pane': 30}: sys.exit('FAIL: resource row rejected')
    for bad in [[], [floor, floor], [dict(floor, id='x')], [dict(floor, seq=0)],
                [dict(floor, seq=True)], [dict(floor, seq='32')], [floor, dict(pane, seq=33)],
                [floor, pane, pane], {'rows': [floor]}]:
        if not rejected(summarize, bad, 'w1', 'w1:p1'): sys.exit('FAIL: invalid checkpoint accepted')
    first = {'endpoint': 32, 'pane': 30}
    for good in [first, {'endpoint': 64, 'pane': 30}, {'endpoint': 64, 'pane': 31, 'workspace': 2}]:
        if not non_decreasing(first, good): sys.exit('FAIL: unchanged or advanced checkpoint rejected')
    for bad in [{'endpoint': 31, 'pane': 30}, {'endpoint': 32, 'pane': 29}, {'endpoint': 32}]:
        if non_decreasing(first, bad): sys.exit('FAIL: regressed checkpoint accepted')
    print('PASS: checkpoint validation accepts endpoint-only/unchanged floors, rejects missing/duplicate/invalid/regressed rows')
elif sys.argv[1] == 'record':
    paths = glob.glob(sys.argv[2] + '/session-*/sequences.json')
    if len(paths) != 1: sys.exit('Expected one isolated sequence checkpoint')
    try: print(json.dumps(summarize(json.load(open(paths[0])), *sys.argv[3:5]), sort_keys=True))
    except ValueError as error: sys.exit(f'Invalid sequence checkpoint: {error}')
else:
    first, second = [json.load(open(p)) for p in sys.argv[2:4]]
    if not non_decreasing(first, second): sys.exit('Sequence checkpoint regressed between samples')
    print('Sequence checkpoint: ' + json.dumps(first, sort_keys=True) + ' -> ' + json.dumps(second, sort_keys=True))
PYSEQ
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
    sequence_tool self-test
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
        CODEX_HOME="$home/.codex" GROK_HOME="$home/.grok" KIMI_CODE_HOME="$home/.kimi-code" \
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
    $found || {
        token_check explain "$temp/snapshot.json" "$workspace" "$pane" "$ship_unix" >&2 || true
        echo 'Expected workspace/pane lineup_ tokens missing or incorrect' >&2
        exit 1
    }
    python3 - "$temp/snapshot.json" "$workspace" "$pane" <<'PYVIEW'
import json, sys
rows=json.load(open(sys.argv[1]))['result']['snapshot']
for kind, id_key, target in [('workspaces','workspace_id',sys.argv[2]),('panes','pane_id',sys.argv[3])]:
    row=next(r for r in rows[kind] if r[id_key]==target)
    print(kind + ': ' + json.dumps({k:v for k,v in row.get('tokens',{}).items() if k.startswith('lineup_')}, ensure_ascii=False, sort_keys=True))
PYVIEW
    echo 'PASS: workspace heading and synthetic pane display tokens received'
    echo 'SKIP: real agent pane; isolated HOME has no provider authentication; synthetic occupant only'
    sequence_tool record "$temp/state" "$workspace" "$pane" > "$temp/seq-first.json"
    # The default TTL is 15 seconds; unchanged keys renew by 10 seconds.
    # Tokens in a snapshot taken 16 seconds later prove renewal; the checkpoint
    # need only keep a valid, non-decreasing endpoint floor.
    sleep 16
    herdr_cmd api snapshot > "$temp/renewed.json"
    if ! token_check present "$temp/renewed.json" "$workspace" "$pane" "$ship_unix"; then
        token_check explain "$temp/renewed.json" "$workspace" "$pane" "$ship_unix" >&2 || true
        echo 'Tokens expired before renewal' >&2
        exit 1
    fi
    sequence_tool record "$temp/state" "$workspace" "$pane" > "$temp/seq-second.json"
    sequence_tool check "$temp/seq-first.json" "$temp/seq-second.json"
    echo 'PASS: tokens retained after default TTL; checkpoint floor non-decreasing'
else
    echo 'SKIP: publication tokens; rerun with --tokens to verify published sidebar metadata'
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
