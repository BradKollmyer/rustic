#!/bin/bash
# macOS read-only live restore benchmark. Credentials stay in external files.
set -euo pipefail

label=${1:?usage: run.sh LABEL BINARY CONNECTIONS [GLOB]}
binary=${2:?binary required}
export STORJ_BENCH_CONNECTIONS=${3:?connections required}
pattern=${4:-7P5A35??.CR3}
: "${STORJ_BENCH_ROOT:?set the directory for fresh benchmark outputs}"
: "${STORJ_BENCH_REFERENCE:?set the verified reference photo directory}"
: "${STORJ_BENCH_PASSWORD_FILE:?set the repository password file}"
: "${STORJ_BENCH_ACCESS_FILE:?set the Storj access grant file}"
[[ $label =~ ^[a-zA-Z0-9_-]+$ ]] || exit 2
[[ $STORJ_BENCH_CONNECTIONS =~ ^[1-9][0-9]*$ ]] || exit 2
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
run_dir=$(mktemp -d "$STORJ_BENCH_ROOT/$label.XXXXXX")
printf 'Run directory: %s\n' "$run_dir"
ulimit -n 8192
"$binary" --version > "$run_dir/version.txt"
date -u '+%Y-%m-%dT%H:%M:%SZ' > "$run_dir/start.txt"
/usr/bin/time -l -p -o "$run_dir/time.txt" \
  "$binary" -P "$script_dir/profile" --profile-substitute-env \
  restore 59bff332:/var/mnt/photos/2026-08-31 "$run_dir/2026-08-31" \
  --no-ownership --glob "$pattern" \
  --log-file "$run_dir/restore.log" --json-progress --progress-interval 15s \
  > "$run_dir/progress.jsonl" 2> "$run_dir/console.log" &
timer_pid=$!
printf '%s\n' "$timer_pid" > "$run_dir/timer.pid"
restore_status=0
wait "$timer_pid" || restore_status=$?
printf 'restore_exit=%s\n' "$restore_status" > "$run_dir/result.txt"
date -u '+%Y-%m-%dT%H:%M:%SZ' > "$run_dir/end.txt"
if [[ $restore_status -ne 0 ]]; then
  tail -n 20 "$run_dir/console.log"
  exit "$restore_status"
fi

count=0
bytes=0
shopt -s nullglob
for reference in "$STORJ_BENCH_REFERENCE"/$pattern; do
  [[ -f $reference ]] || continue
  restored="$run_dir/2026-08-31/${reference##*/}"
  cmp -s "$reference" "$restored"
  count=$((count + 1))
  bytes=$((bytes + $(stat -f '%z' "$restored")))
done
actual_count=0
for restored in "$run_dir/2026-08-31"/*; do
  [[ -f $restored ]] || continue
  actual_count=$((actual_count + 1))
done
[[ $count -gt 0 && $actual_count -eq $count ]]
printf 'verified_files=%s\nverified_bytes=%s\n' "$count" "$bytes" >> "$run_dir/result.txt"
sed -n '1,20p' "$run_dir/time.txt" "$run_dir/result.txt"
