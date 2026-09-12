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

Keep message timeout20s, hedge delay1s, metadata cache, file descriptor limit8192,
binary build settings and ownership handling constant unless explicitly varied.
Warm metadata with a dry run before timing. Use repeated/interleaved subset
measurements before full-day confirmation; report regressions and network
variability, not only the fastest sample. Each completed experiment gets a commit.

Historical full day: original native4748s, hedged native1151.26s,
OpenDAL gateway689.17s; all814 files/15,107,783,814 bytes matched.
These historical runs were sequential, not controlled simultaneous measurements.

The old temporary native profile/cache was no longer present during setup;
the initial dry run failed before opening a repository. The benchmark now uses
an environment-substituted profile and a persistent output/cache directory.
The first new-cache warmup omitted the intended descriptor limit and failed
with `Too many open files`; it was excluded and repeated with `ulimit -n 8192`.
The runner sets the limit internally so timed measurements cannot omit it.

## Results

### Test01: fresh baseline, five connections

`baseline-c5-a.jmMqpa`: wall160.92s, CPU15.05s user +5.03s system,
10.96MiB/s payload throughput. All100 files/1,849,631,128 bytes verified
with `cmp`; restore and runner exited0. No restore warnings.

This run included a10-second `sample` (5ms interval) and ten1Hz `nettop`
snapshots. The stripped binary limits symbol detail, but top-of-stack samples
were dominated by condition-variable waits (102,045), semaphore waits (3,650)
and `kevent` (1,703), not active CPU work. Profiling overhead is included;
later comparisons will include an unprofiled baseline repeat.

Artifacts and preserved baseline binary are under
`/Users/bradk/repos/rustic/restore/native-iterations.RsMRDY/`.

### Candidate: fixed hedge launch cadence

For these60MiB packs, multi-segment prefetch is unlikely to help: they fit
within one64MiB Storj segment. The current scheduler recreates its1s sleep
after every completion, so sub-second trickling successes postpone spares.
Test a persistent launch deadline while retaining the same35-attempt production
speculative cap, immediate failure replacements and cancellation safeguards.

### Test02: fixed cadence, five connections

SDK `f0e18e6`; release binary stamp `0.11.4-native-fixed-hedge-iteration1`.
Built with the same release features, LTO disabled and16 codegen units as the
baseline. The CLI patch points to the sibling SDK experiment worktree; core
code remains pinned to `acd990e`.

`fixed-c5-a.ShCTod`: wall100.88s, CPU14.79s user +5.07s system,
17.49MiB/s payload throughput. All100 files/1,849,631,128 bytes verified;
restore and runner exited0, no warnings. This is1.60× faster than Test01
(37.3% less wall time), but needs an unprofiled/interleaved baseline repeat.

SDK regression validation: the first spare now launches at1s rather than25.3s
under simulated trickling completions, with no change to the35-attempt cap.
136 SDK unit/API/mock tests pass, one pre-existing ignored test. The initial
sandbox suite's unrelated loopback-bind failure passed with socket permission.

### Test03: unprofiled baseline repeat, five connections

`baseline-c5-b.Uswlr2`: wall154.03s, CPU15.09s user +5.32s system.
All100 files/1,849,631,128 bytes verified; restore and runner exited0,
no warnings. No sampling or concurrent compilation. This is close to the
profiled baseline160.92s; the candidate's100.88s remains1.53× faster than
this unprofiled repeat. Next: repeat the candidate before concurrency tuning.

### Test04: fixed-cadence repeat, five connections

`fixed-c5-b.OMUT6K`: wall100.31s, CPU14.92s user +5.20s system.
All100 files/1,849,631,128 bytes verified; restore and runner exited0,
no warnings. No sampling or concurrent compilation.

Interleaved wall times: baseline160.92s, candidate100.88s, baseline154.03s,
candidate100.31s. The two candidate timings differ by only0.57s. Mean baseline
157.48s versus candidate100.60s is a1.57× speedup (36.1% less time), with
the caveat that the first baseline included brief profiling. Next experiment
changes only the candidate's connection count from5 to10.

### Test05: fixed cadence, ten connections

`fixed-c10-a.ROYpw2`: wall53.99s, CPU15.28s user +5.64s system,
32.67MiB/s payload throughput. All100 files/1,849,631,128 bytes verified;
restore and runner exited0, no warnings. Maximum RSS1,951,154,176 bytes,
versus1,521,319,936 bytes for Test04. Ten connections improved throughput
substantially without CPU saturation. The SDK pool includes idle+active
connections in its cap (1100 for this setting); no pool changes were made.

Next test uses the existing restore ceiling of20 connections, still with
the8192 descriptor limit and the same bounded scheduler. This is an explicit
benchmark setting, not a change to the backend default of5.

### Test06: fixed cadence, twenty connections

`fixed-c20-a.T5765p`: wall35.56s, CPU15.01s user +5.21s system,
49.60MiB/s payload throughput. All100 files/1,849,631,128 bytes verified;
restore and runner exited0, no warnings. The same binary at20 connections
is1.52× faster than Test05 at10. Repeat10 then20 before selecting the
full-day confirmation setting; do not infer a general default from one Mac.
Maximum RSS was2,884,304,896 bytes (peak memory footprint2,328,873,744).

### Test07: ten-connection repeat

`fixed-c10-b.JzJy2g`: wall61.10s, CPU15.89s user +5.63s system.
All100 files/1,849,631,128 bytes verified; restore and runner exited0,
no warnings. This repeat is slower than the53.99s first run, showing real
network variance, but remains substantially faster than five connections.

### Test08: twenty-connection repeat

`fixed-c20-b.TBAjD1`: wall32.61s, CPU15.19s user +5.33s system.
All100 files/1,849,631,128 bytes verified; restore and runner exited0,
no warnings. This confirms the35.56s first run. All eight completed subset
restores matched the reference byte for byte.

| Scheduler | Connections | First run | Repeat | Mean wall |
|---|---:|---:|---:|---:|
| Resettable hedge timer | 5 | 160.92s | 154.03s | 157.48s |
| Fixed hedge cadence | 5 | 100.88s | 100.31s | 100.60s |
| Fixed hedge cadence | 10 | 53.99s | 61.10s | 57.55s |
| Fixed hedge cadence | 20 | 35.56s | 32.61s | 34.09s |

At equal concurrency, the scheduler change is1.57× faster in these samples.
Combined with20 connections it is4.62× faster than the original five-connection
baseline. These are different comparisons: the latter includes concurrency
tuning, not just a code improvement. Select20 for full-day confirmation, while
leaving the shipped default of5 unchanged.
