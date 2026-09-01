//! `warmup` subcommand

use crate::{Application, RUSTIC_APP, repository::IndexedRepo, status_err};

use abscissa_core::{Command, Runnable, Shutdown};
use anyhow::Result;
use rustic_core::LsOptions;

use crate::filtering::SnapshotFilter;

/// `warmup` subcommand
#[derive(clap::Parser, Command, Debug)]
pub(crate) struct WarmupCmd {
    /// Snapshot/path whose data packs should be warmed
    ///
    /// Snapshot can be identified the following ways: "01a2b3c4" or "latest" or "latest~N" (N >= 0)
    #[clap(value_name = "SNAPSHOT[:PATH]")]
    snap: String,

    /// Wait until warmup finishes (uses `--warm-up-wait` / `--warm-up-wait-command`)
    #[clap(long)]
    wait: bool,

    /// Snapshot filter options (when using latest)
    #[clap(
        flatten,
        next_help_heading = "Snapshot filter options (when using latest)"
    )]
    filter: SnapshotFilter,
}

impl Runnable for WarmupCmd {
    fn run(&self) {
        if let Err(err) = RUSTIC_APP
            .config()
            .repository
            .run_indexed(|repo| self.inner_run(repo))
        {
            status_err!("{}", err);
            RUSTIC_APP.shutdown(Shutdown::Crash);
        };
    }
}

impl WarmupCmd {
    fn inner_run(&self, repo: IndexedRepo) -> Result<()> {
        let config = RUSTIC_APP.config();
        let node =
            repo.node_from_snapshot_path(&self.snap, |sn| config.snapshot_filter.matches(sn))?;

        let mut ls_opts = LsOptions::default();
        ls_opts.recursive = true;
        let ls = repo.ls(&node, &ls_opts)?;
        let packs = repo.packs_for_nodes(ls)?;

        println!("warming up {} pack(s)", packs.len());

        if self.wait {
            repo.warm_up_wait(packs.into_iter())?;
        } else {
            repo.warm_up(packs.into_iter())?;
        }
        Ok(())
    }
}
