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
