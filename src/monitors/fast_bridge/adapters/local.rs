//! What is read from this machine: the provider's log and its reverse stock through commands, its swap records from disk.

use super::wire;
use crate::monitors::{
    common::local::{follow, json_records, not_found},
    fast_bridge::{
        model::{Level, LogEntry, ReverseStock, Swap},
        ports::{Fut, Inventory, LogFeed, SilentCheck, SwapStore},
    },
};
use anyhow::{Context, bail};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, sync::mpsc::Receiver};

pub struct CommandLog {
    cmd: Vec<String>,
}

pub fn create_command_log(cmd: &[String]) -> CommandLog {
    CommandLog { cmd: cmd.to_vec() }
}

impl LogFeed for CommandLog {
    fn start(self: Box<Self>) -> Receiver<LogEntry> {
        follow(self.cmd, wire::log_line, |why| LogEntry::plain(Level::Warn, why))
    }
}

/// The most of the status command's output that is read. It prints a couple of kilobytes.
const MAX_STATUS: u64 = 64 << 10;
/// How long one status run may take. It scans the pool, which takes the better part of a minute.
const STATUS_WAIT: Duration = Duration::from_secs(180);

pub struct CommandInventory {
    cmd: Vec<String>,
}

pub fn create_command_inventory(cmd: &[String]) -> CommandInventory {
    CommandInventory { cmd: cmd.to_vec() }
}

/// One run of a command that holds the key, with `more` after its own arguments: whether it succeeded, and what it
/// printed. This process only reads that.
async fn run(cmd: &[String], more: &[&str], wait: Duration) -> anyhow::Result<(bool, String)> {
    let Some((prog, args)) = cmd.split_first() else { bail!("the command is empty") };
    // no stdin: a sudo inside cannot stop to ask for a password, it fails instead
    let mut child = tokio::process::Command::new(prog)
        .args(args)
        .args(more)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut out = Vec::new();
    let stdout = child.stdout.take().context("no stdout")?;
    let read = async {
        stdout.take(MAX_STATUS).read_to_end(&mut out).await?;
        child.wait().await
    };
    let status = tokio::time::timeout(wait, read).await.context("the command took too long")??;
    Ok((status.success(), String::from_utf8_lossy(&out).into_owned()))
}

impl Inventory for CommandInventory {
    fn reverse_stock(&self) -> Fut<'_, ReverseStock> {
        Box::pin(async move {
            let (ok, out) = run(&self.cmd, &[], STATUS_WAIT).await?;
            if !ok {
                bail!("status_cmd failed");
            }
            wire::reverse_stock(&out).context("status_cmd printed no reverse address and TAC")
        })
    }
}

/// A dry run reads the transaction, then scans what the key holds: slower than a status run.
const NORMALIZE_WAIT: Duration = Duration::from_secs(600);

pub struct CommandSilent {
    cmd: Vec<String>,
}

pub fn create_command_silent(cmd: &[String]) -> CommandSilent {
    CommandSilent { cmd: cmd.to_vec() }
}

impl SilentCheck for CommandSilent {
    /// `--txid <id>` and nothing more: without `--send` the tool moves nothing.
    fn waiting<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<u128>> {
        Box::pin(async move {
            // the id comes from an explorer's list, and goes onto a command line
            let id = wire::txid_text(txid).context("not a transaction id")?;
            // the tool exits with a failure when there is nothing to move: what it printed says which kind
            let (_, out) = run(&self.cmd, &["--txid", &id], NORMALIZE_WAIT).await?;
            wire::silent_verdict(&out).context("normalize_cmd did not say")
        })
    }
}

pub struct FileSwaps {
    dir: PathBuf,
}

pub fn create_file_swaps(data_dir: &std::path::Path) -> FileSwaps {
    FileSwaps { dir: data_dir.to_path_buf() }
}

impl SwapStore for FileSwaps {
    /// Both directions. `reverse/swaps/` is only there once the reverse side has run: its absence is no records, any
    /// other failure to read it a failed read, as for `swaps/`.
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move {
            let (mut out, mut seen) = (Vec::new(), 0usize);
            json_records(&self.dir.join("swaps"), wire::swap, &mut seen, &mut out).await?;
            match json_records(&self.dir.join("reverse/swaps"), wire::reverse_swap, &mut seen, &mut out).await {
                Err(e) if not_found(&e) => {}
                other => other?,
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::{common::local::MAX_RECORD, fast_bridge::model::Direction};

    #[cfg(unix)]
    #[tokio::test]
    async fn only_ordinary_files_of_a_records_size_are_read() {
        let dir = std::env::temp_dir().join(format!("tacmon-test-{}", std::process::id()));
        let swaps = dir.join("swaps");
        std::fs::create_dir_all(&swaps).expect("a directory");
        let good = r#"{"id":"aa","phase":"done","offer":{"btc":{"value":"2"},"evm":{"amount":"1"}}}"#;
        std::fs::write(swaps.join("good.json"), good).expect("a record");
        std::fs::write(dir.join("elsewhere.json"), good).expect("a file outside");
        std::os::unix::fs::symlink(dir.join("elsewhere.json"), swaps.join("link.json")).expect("a link");
        // the files beside a record are not records: they must not count toward the limit or be read
        std::fs::write(swaps.join("good.json.bak"), good).expect("a backup beside the record");
        std::fs::write(swaps.join("huge.json"), vec![b' '; usize::try_from(MAX_RECORD).expect("fits") + 1]).expect("a large file");

        let read = create_file_swaps(&dir).swaps().await.expect("a read");

        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["aa"]);
    }

    #[tokio::test]
    async fn both_directions_are_read_and_a_reverse_side_that_never_ran_has_no_records() {
        let dir = std::env::temp_dir().join(format!("tacmon-test-both-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("swaps")).expect("a directory");
        std::fs::write(dir.join("swaps/f.json"), r#"{"id":"fw","phase":"done","offer":{"btc":{"value":"2"},"evm":{"amount":"1"}}}"#).expect("a record");
        let forward_only = create_file_swaps(&dir).swaps().await.expect("a read").len();
        std::fs::create_dir_all(dir.join("reverse/swaps")).expect("a directory");
        std::fs::write(dir.join("reverse/swaps/r.json"), r#"{"id":"rv","phase":"built","amount":"5","evmAmount":"6"}"#).expect("a record");

        let mut read = create_file_swaps(&dir).swaps().await.expect("a read");

        let _ = std::fs::remove_dir_all(&dir);
        read.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(forward_only, 1);
        assert_eq!(read.iter().map(|s| (s.id.as_str(), s.direction)).collect::<Vec<_>>(), [("fw", Direction::Forward), ("rv", Direction::Reverse)]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_status_command_that_fails_or_prints_nothing_useful_is_a_failed_read() {
        let run = |cmd: &[&str]| create_command_inventory(&cmd.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let good = "printf 'reverse address   bc1qtestreverse00000000000000000000000000\\nTAC at it  1 confirmed note, 20 TAC in all\\n'";

        let (ok, failed, silent) = (
            run(&["sh", "-c", good]).reverse_stock().await,
            run(&["sh", "-c", &format!("{good}; exit 3")]).reverse_stock().await,
            run(&["true"]).reverse_stock().await,
        );

        assert_eq!(ok.expect("a read").tac, 20 * crate::monitors::fast_bridge::model::UNIT);
        assert!(failed.is_err() && silent.is_err());
    }
}
