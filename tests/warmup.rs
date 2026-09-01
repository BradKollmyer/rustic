//! CLI tests for `warmup`, `--warmup-only`, and `--require-warm`.

use assert_cmd::Command;
use predicates::prelude::predicate;
use rustic_testing::TestResult;
use tempfile::{TempDir, tempdir};

fn rustic_runner(temp_dir: &TempDir) -> TestResult<Command> {
    let mut runner = Command::new(env!("CARGO_BIN_EXE_rustic"));
    runner.arg("-r").arg(temp_dir.path().join("repo")).args([
        "--password",
        "test",
        "--no-progress",
    ]);
    Ok(runner)
}

fn setup_with_backup() -> TestResult<TempDir> {
    let temp_dir = tempdir()?;
    rustic_runner(&temp_dir)?.arg("init").assert().success();

    let source = temp_dir.path().join("source");
    std::fs::create_dir(&source)?;
    std::fs::write(source.join("file.txt"), b"hello glacier")?;

    rustic_runner(&temp_dir)?
        .arg("backup")
        .arg(&source)
        .assert()
        .success();

    Ok(temp_dir)
}

#[test]
fn warmup_lists_packs_for_latest_snapshot() -> TestResult<()> {
    let temp_dir = setup_with_backup()?;
    rustic_runner(&temp_dir)?
        .args(["warmup", "latest"])
        .assert()
        .success()
        .stdout(predicate::str::contains("warming up"))
        .stdout(predicate::str::contains("pack"));
    Ok(())
}

#[test]
fn warmup_status_on_local_repo_reports_all_warm() -> TestResult<()> {
    let temp_dir = setup_with_backup()?;
    rustic_runner(&temp_dir)?
        .args(["warmup", "--status", "latest"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cold=0"));
    Ok(())
}

#[test]
fn restore_warmup_only_does_not_extract() -> TestResult<()> {
    let temp_dir = setup_with_backup()?;
    rustic_runner(&temp_dir)?
        .args(["restore", "--warmup-only", "latest"])
        .assert()
        .success()
        .stdout(predicate::str::contains("warming up"));
    Ok(())
}

#[test]
fn require_warm_fails_when_backend_cannot_report_status() -> TestResult<()> {
    let temp_dir = setup_with_backup()?;
    let dest = temp_dir.path().join("out");
    rustic_runner(&temp_dir)?
        .args(["restore", "--require-warm", "latest"])
        .arg(&dest)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot check warmup status"));
    Ok(())
}

#[test]
fn dump_require_warm_fails_on_local_backend() -> TestResult<()> {
    let temp_dir = setup_with_backup()?;
    rustic_runner(&temp_dir)?
        .args(["dump", "--require-warm", "latest"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot check warmup status"));
    Ok(())
}
