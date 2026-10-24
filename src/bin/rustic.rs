//! Main entry point for Rustic

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]
#![allow(unsafe_code)]

#[cfg(all(feature = "mimalloc", feature = "jemallocator"))]
compile_error!(
    "feature \"mimalloc\" and feature \"jemallocator\" cannot be enabled at the same time. Please disable one of them."
);

#[cfg(feature = "mimalloc")]
use mimalloc::MiMalloc;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

// jemallocator-global registers the #[global_allocator] from within the crate, so it has
// to be referenced here: an unused dependency is not linked into the binary and the
// allocator would silently stay the default one.
#[cfg(feature = "jemallocator")]
use jemallocator_global as _;

use rustic_rs::application::RUSTIC_APP;

/// Boot Rustic
fn main() {
    // systemd's default LimitNOFILE is 1024:524288 (soft:hard). The kernel
    // enforces the soft limit. Storj opens one socket per piece, so a parallel
    // repack exceeds 1024 while the hard limit still has room. Raise soft to
    // hard before any command opens files. A no-op when they are already equal.
    raise_nofile_soft_to_hard_limit();
    abscissa_core::boot(&RUSTIC_APP);
}

/// Soft open-file limit to install when `soft` is below `hard`.
///
/// `None` when the soft limit is already at the hard limit.
fn nofile_soft_target<T: Copy + Ord>(soft: T, hard: T) -> Option<T> {
    (soft < hard).then_some(hard)
}

/// Raise the soft `RLIMIT_NOFILE` to the hard limit.
///
/// Best-effort. A failed `setrlimit` leaves the current soft limit in place
/// and startup continues. Nothing to do when the soft limit is already at the
/// hard limit, or on non-Unix targets.
fn raise_nofile_soft_to_hard_limit() {
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // Safety: `limit` is a valid `rlimit`, and `RLIMIT_NOFILE` is a real resource.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            return;
        }
        let Some(soft) = nofile_soft_target(limit.rlim_cur, limit.rlim_max) else {
            return;
        };
        limit.rlim_cur = soft;
        // Safety: same `rlimit`. Failure is ignored; the process keeps its soft limit.
        _ = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) };
    }
}

#[cfg(test)]
mod tests {
    use super::nofile_soft_target;
    #[cfg(unix)]
    use super::raise_nofile_soft_to_hard_limit;

    #[test]
    fn nofile_soft_target_raises_only_when_below_hard() {
        // systemd's default split, an already-raised limit, and a soft limit
        // that is already above the hard limit.
        assert_eq!(nofile_soft_target(1024_u64, 524_288), Some(524_288));
        assert_eq!(nofile_soft_target(524_288_u64, 524_288), None);
        assert_eq!(nofile_soft_target(2048_u64, 1024), None);
    }

    #[cfg(unix)]
    #[test]
    fn raise_nofile_soft_to_hard_limit_reaches_hard() {
        const CHILD: &str = "RUSTIC_NOFILE_RAISE_TEST";
        if std::env::var_os(CHILD).is_none() {
            let name = std::thread::current()
                .name()
                .expect("test thread is named")
                .to_owned();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // Safety: `limit` is a valid `rlimit` for this process.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
        // Drop the hard ceiling in this child so the assertion does not depend
        // on the host's hard limit, including `RLIM_INFINITY`.
        const LOWERED_HARD: libc::rlim_t = 256;
        const LOWERED_SOFT: libc::rlim_t = 64;
        let lowered_hard = limit.rlim_max.min(LOWERED_HARD);
        limit.rlim_max = lowered_hard;
        limit.rlim_cur = lowered_hard.min(LOWERED_SOFT);
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
            0,
            "{}",
            std::io::Error::last_os_error()
        );

        raise_nofile_soft_to_hard_limit();

        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
        assert_eq!(limit.rlim_cur, lowered_hard);
        assert_eq!(limit.rlim_max, lowered_hard);
    }
}
