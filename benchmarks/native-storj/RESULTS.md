# Native Storj performance iterations

Worktree branch: `perf/native-storj-iterations` in the SDK, rustic_core and CLI.
Starting implementations: SDK `e635fba`, core `9fbb124` (code `acd990e`),
CLI `edfc1ca`. Existing checkouts and their uncommitted changes are preserved.

## Protocol

Use snapshot `59bff332`, directory `/var/mnt/photos/2026-08-31`, on this Mac.
The fixed subset `7P5A35??.CR3` contains 100 files and 1,849,631,128 bytes
(1.723 GiB). Restore to a new directory for every measurement, verify every
output byte against the previously verified full-day native output, and retain
logs. Credentials remain in external files. No repository writes are performed.

Keep message timeout 20 s, hedge delay 1 s, metadata cache, file descriptor limit 8192,
binary build settings and ownership handling constant unless explicitly varied.
Warm metadata with a dry run before timing. Use repeated/interleaved subset
measurements before full-day confirmation; report regressions and network
variability, not only the fastest sample. Each completed experiment gets a commit.

Historical full day: original native 4748 s, hedged native 1151.26 s,
OpenDAL gateway 689.17 s; all 814 files/15,107,783,814 bytes matched.
These historical runs were sequential, not controlled simultaneous measurements.

The old temporary native profile/cache was no longer present during setup;
the initial dry run failed before opening a repository. The benchmark now uses
an environment-substituted profile and a persistent output/cache directory.
The first new-cache warmup omitted the intended descriptor limit and failed
with `Too many open files`; it was excluded and repeated with `ulimit -n 8192`.
The runner sets the limit internally so timed measurements cannot omit it.

## Results

### Test 01: fresh baseline, five connections

`baseline-c5-a.jmMqpa`: wall 160.92 s, CPU 15.05 s user + 5.03 s system,
10.96 MiB/s payload throughput. All 100 files/1,849,631,128 bytes verified
with `cmp`; restore and runner exited 0. No restore warnings.

This run included a 10-second `sample` (5 ms interval) and ten 1 Hz `nettop`
snapshots. The stripped binary limits symbol detail, but top-of-stack samples
were dominated by condition-variable waits (102,045), semaphore waits (3,650)
and `kevent` (1,703), not active CPU work. Profiling overhead is included;
later comparisons will include an unprofiled baseline repeat.

Artifacts and preserved baseline binary are under
`/Users/bradk/repos/rustic/restore/native-iterations.RsMRDY/`.

### Candidate: fixed hedge launch cadence

For these 60 MiB packs, multi-segment prefetch is unlikely to help: they fit
within one 64 MiB Storj segment. The current scheduler recreates its 1 s sleep
after every completion, so sub-second trickling successes postpone spares.
Test a persistent launch deadline while retaining the same 35-attempt production
speculative cap, immediate failure replacements and cancellation safeguards.

### Test 02: fixed cadence, five connections

SDK `f0e18e6`; release binary stamp `0.11.4-native-fixed-hedge-iteration1`.
Built with the same release features, LTO disabled and 16 codegen units as the
baseline. The CLI patch points to the sibling SDK experiment worktree; core
code remains pinned to `acd990e`.

`fixed-c5-a.ShCTod`: wall 100.88 s, CPU 14.79 s user + 5.07 s system,
17.49 MiB/s payload throughput. All 100 files/1,849,631,128 bytes verified;
restore and runner exited 0, no warnings. This is 1.60× faster than Test 01
(37.3% less wall time), but needs an unprofiled/interleaved baseline repeat.

SDK regression validation: the first spare now launches at 1 s rather than 25.3 s
under simulated trickling completions, with no change to the 35-attempt cap.
136 SDK unit/API/mock tests pass, one pre-existing ignored test. The initial
sandbox suite's unrelated loopback-bind failure passed with socket permission.

### Test 03: unprofiled baseline repeat, five connections

`baseline-c5-b.Uswlr2`: wall 154.03 s, CPU 15.09 s user + 5.32 s system.
All 100 files/1,849,631,128 bytes verified; restore and runner exited 0,
no warnings. No sampling or concurrent compilation. This is close to the
profiled baseline 160.92 s; the candidate's 100.88 s remains 1.53× faster than
this unprofiled repeat. Next: repeat the candidate before concurrency tuning.

### Test 04: fixed-cadence repeat, five connections

`fixed-c5-b.OMUT6K`: wall 100.31 s, CPU 14.92 s user + 5.20 s system.
All 100 files/1,849,631,128 bytes verified; restore and runner exited 0,
no warnings. No sampling or concurrent compilation.

Interleaved wall times: baseline 160.92 s, candidate 100.88 s, baseline 154.03 s,
candidate 100.31 s. The two candidate timings differ by only 0.57 s. Mean baseline
157.48 s versus candidate 100.60 s is a 1.57× speedup (36.1% less time), with
the caveat that the first baseline included brief profiling. Next experiment
changes only the candidate's connection count from 5 to 10.

### Test 05: fixed cadence, ten connections

`fixed-c10-a.ROYpw2`: wall 53.99 s, CPU 15.28 s user + 5.64 s system,
32.67 MiB/s payload throughput. All 100 files/1,849,631,128 bytes verified;
restore and runner exited 0, no warnings. Maximum RSS 1,951,154,176 bytes,
versus 1,521,319,936 bytes for Test 04. Ten connections improved throughput
substantially without CPU saturation. The SDK pool includes idle+active
connections in its cap (1100 for this setting); no pool changes were made.

Next test uses the existing restore ceiling of 20 connections, still with
the 8192 descriptor limit and the same bounded scheduler. This is an explicit
benchmark setting, not a change to the backend default of 5.

### Test 06: fixed cadence, twenty connections

`fixed-c20-a.T5765p`: wall 35.56 s, CPU 15.01 s user + 5.21 s system,
49.60 MiB/s payload throughput. All 100 files/1,849,631,128 bytes verified;
restore and runner exited 0, no warnings. The same binary at 20 connections
is 1.52× faster than Test 05 at 10. Repeat 10 then 20 before selecting the
full-day confirmation setting; do not infer a general default from one Mac.
Maximum RSS was 2,884,304,896 bytes (peak memory footprint 2,328,873,744).

### Test 07: ten-connection repeat

`fixed-c10-b.JzJy2g`: wall 61.10 s, CPU 15.89 s user + 5.63 s system.
All 100 files/1,849,631,128 bytes verified; restore and runner exited 0,
no warnings. This repeat is slower than the 53.99 s first run, showing real
network variance, but remains substantially faster than five connections.

### Test 08: twenty-connection repeat

`fixed-c20-b.TBAjD1`: wall 32.61 s, CPU 15.19 s user + 5.33 s system.
All 100 files/1,849,631,128 bytes verified; restore and runner exited 0,
no warnings. This confirms the 35.56 s first run. All eight completed subset
restores matched the reference byte for byte.

| Scheduler | Connections | First run | Repeat | Mean wall |
|---|---:|---:|---:|---:|
| Resettable hedge timer | 5 | 160.92 s | 154.03 s | 157.48 s |
| Fixed hedge cadence | 5 | 100.88 s | 100.31 s | 100.60 s |
| Fixed hedge cadence | 10 | 53.99 s | 61.10 s | 57.55 s |
| Fixed hedge cadence | 20 | 35.56 s | 32.61 s | 34.09 s |

At equal concurrency, the scheduler change is 1.57× faster in these samples.
Combined with 20 connections it is 4.62× faster than the original five-connection
baseline. These are different comparisons: the latter includes concurrency
tuning, not just a code improvement. Select 20 for full-day confirmation, while
leaving the shipped default of 5 unchanged.

### Test 09: full-day confirmation, twenty connections

`fixed-c20-full.KixzjN`: **201.46 s (3.36 min)** wall,
**71.52 MiB/s** payload throughput, CPU 103.16 s user + 40.45 s system.
Restore and runner exited 0, with no restore warnings. All **814 files**,
**15,107,783,814 bytes (14.07 GiB)**, matched the verified reference byte for
byte using `cmp`; the output file count also matched. Verification runs after
the timed restore, consistently with the subset measurements.

Maximum RSS was **3,952,066,560 bytes**; peak memory footprint was
3,104,001,600 bytes, with zero swaps reported by `time`. No compilation,
sampling or other benchmark ran concurrently. The same warm metadata cache,
snapshot, message timeout, hedge interval and ownership setting were retained.

| Full-day run | Object connections | Wall | Payload throughput |
|---|---:|---:|---:|
| Earlier native, resettable hedge timer | 5 | 19.19 min | 12.51 MiB/s |
| Earlier OpenDAL S3 gateway | 5 | 11.49 min | 20.91 MiB/s |
| Fixed-cadence native, tuned | 20 | **3.36 min** | **71.52 MiB/s** |

The tuned native run is **5.71× faster** than the earlier native full-day run,
saving **15.83 minutes (82.5% less time)**. It also finished sooner than the
earlier gateway run, but **20 versus 5 connections is not an equal-concurrency
backend comparison**. Gateway performance at 20 connections was not tested.
Full-day comparisons are historical/sequential single runs; the stronger
evidence isolating the code change is the interleaved five-connection subset.

## Handoff

The SDK code change is `f0e18e6`; subsequent SDK edits only clarify documentation.
136 SDK unit/API/mock tests pass, with one pre-existing ignored test. SDK
Clippy passes for all targets with warnings denied; formatting and diff checks
pass. The CLI built offline in release mode using the sibling SDK worktree,
with the same release features, disabled LTO and 16 codegen units as baseline.

For publishing, the CLI now pins SDK commit
`a2f680843d560f2e72a5e03908032f81d60da6c0` from the GitHub fork instead of
the sibling SDK checkout. It contains the same tested scheduler code plus
documentation updates; new builds do not require the local SDK worktree.
`cargo check --features release --bin rustic` passes with this Git dependency.

All nine live test results were committed separately. The original source
checkouts and their unrelated edits remain unchanged; the core worktree was
created for isolation but needed no code changes. Experimental code remains on
the performance branches, not merged or deployed to arc. The backend default
remains five connections.
Twenty is an opt-in setting tested on this Mac with an 8192 descriptor limit;
allow for its higher memory/socket use. Speculative-transfer bytes were not
measured, so the latency improvement is not a claim of unchanged egress cost.

Reproduce on this Mac (each invocation creates a fresh destination):

```sh
cd /Users/bradk/repos/rustic/.worktrees/rustic-native-iterations
export STORJ_BENCH_ROOT=/Users/bradk/repos/rustic/restore/native-iterations.RsMRDY
export STORJ_BENCH_REFERENCE=/Users/bradk/repos/rustic/restore/photos-hedged.r9h5lJ/2026-08-31
export STORJ_BENCH_PASSWORD_FILE=/Users/bradk/repos/rustic/password
export STORJ_BENCH_ACCESS_FILE=/Users/bradk/.config/rustic/storj.grant
bash benchmarks/native-storj/run.sh full-repeat \
  "$STORJ_BENCH_ROOT/native-fixed-hedge" 20 '*'
```

Preserved candidate binary: `$STORJ_BENCH_ROOT/native-fixed-hedge`.
Full-day output, timing, version, binary SHA-256, parameters and verification
receipt: `$STORJ_BENCH_ROOT/fixed-c20-full.KixzjN/`.
