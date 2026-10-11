#!/bin/bash
# Runs two recorders on one selection under the same sandbox and diffs their
# sorted output.
#
#   compare.sh OLD.php NEW.php REGEX [JOBS] [-- NEW-ARGS...]
#
# REGEX is the recorder's first argument (PCRE with delimiters, matched on
# the lowercased builtin name). Both scripts run in one `bwrap` with no
# network, a fresh capped `/tmp` and a fixed environment; NEW-ARGS go to the
# second script only. The outputs are kept in $COMPARE_DIR (default
# ${TMPDIR:-/tmp}/builtin-runs-compare) as old.tsv and new.tsv, sorted with
# LC_ALL=C. $COMPARE_BIND (space-separated directories) are bound writable
# in both runs, for a file a recorder is asked to write (`--stats=`).
# Exit status: 0 equal, 1 different, 2 a recorder failed.
set -u

if [ $# -lt 3 ]; then
    echo "usage: compare.sh OLD.php NEW.php REGEX [JOBS] [-- NEW-ARGS...]" >&2
    exit 2
fi
old=$(realpath "$1")
new=$(realpath "$2")
regex=$3
shift 3
jobs=8
if [ $# -gt 0 ] && [ "$1" != "--" ]; then
    jobs=$1
    shift
fi
if [ $# -gt 0 ] && [ "$1" = "--" ]; then
    shift
fi
new_args=("$@")

dir=${COMPARE_DIR:-${TMPDIR:-/tmp}/builtin-runs-compare}
mkdir -p "$dir"

run() { # script output error [extra args...]
    local script=$1 out=$2 err=$3
    shift 3
    local binds=()
    case "$(dirname "$script")" in
        /tmp/*) binds=(--bind "$(dirname "$script")" "$(dirname "$script")") ;;
    esac
    for extra in ${COMPARE_BIND:-}; do
        binds+=(--bind "$extra" "$extra")
    done
    nice -n 19 bwrap \
        --unshare-net --unshare-pid --unshare-uts --unshare-ipc \
        --die-with-parent --new-session \
        --clearenv --setenv PATH "$PATH" --setenv HOME /tmp --setenv LC_ALL C \
        --setenv TZ UTC --setenv USE_ZEND_ALLOC 0 \
        --ro-bind / / --dev /dev --proc /proc \
        --size 268435456 --tmpfs /tmp "${binds[@]}" \
        --chdir /tmp \
        php "$script" "--jobs=$jobs" "$regex" "$@" >"$out" 2>"$err"
    local status=$?
    LC_ALL=C sort -o "$out" "$out"
    return $status
}

run "$old" "$dir/old.tsv" "$dir/old.err" || { echo "old recorder failed (see $dir/old.err)" >&2; exit 2; }
run "$new" "$dir/new.tsv" "$dir/new.err" "${new_args[@]}" || { echo "new recorder failed (see $dir/new.err)" >&2; exit 2; }

echo "old $(wc -l <"$dir/old.tsv") rows, new $(wc -l <"$dir/new.tsv") rows"
if diff "$dir/old.tsv" "$dir/new.tsv" >"$dir/diff.txt"; then
    echo "equal"
    exit 0
fi
echo "different: $(grep -c '^<' "$dir/diff.txt") only in old, $(grep -c '^>' "$dir/diff.txt") only in new ($dir/diff.txt)"
head -20 "$dir/diff.txt"
exit 1
