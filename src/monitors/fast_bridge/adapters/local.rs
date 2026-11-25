//! What is read from this machine: the provider's log and its reverse stock through commands, its swap records from disk.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Level, LogEntry, ReverseStock, Swap},
    ports::{Fut, Inventory, LogFeed, QUEUE, SilentCheck, SwapStore},
};
use anyhow::{Context, bail};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::mpsc::{Receiver, channel},
};

/// The most of one log line that is kept. The provider's lines are a few hundred bytes.
const MAX_LINE: usize = 64 << 10;
/// The most one swap record may hold. The provider's are a few kilobytes.
const MAX_RECORD: u64 = 1 << 20;
/// The most records read in one pass, counting records only and not the files the store keeps beside each. Past
/// this the read fails: a part of the records would be drawn as all of them.
const MAX_RECORDS: usize = 20_000;
/// How long one record may take to open and read. A pipe put where a record was never opens.
const RECORD_WAIT: Duration = Duration::from_secs(2);

/// One record's bytes, or `None` for anything that is not an ordinary file of a record's size. The size is checked
/// on the open file and the read stops at the limit, so a file changed after the check cannot be read past it.
async fn record(p: &Path) -> Option<Vec<u8>> {
    // a link is not a record: it could point at any file this user can read
    if !tokio::fs::symlink_metadata(p).await.ok()?.is_file() {
        return None;
    }
    let file = tokio::fs::File::open(p).await.ok()?;
    let meta = file.metadata().await.ok()?;
    if !meta.is_file() || meta.len() > MAX_RECORD {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD + 1).read_to_end(&mut bytes).await.ok()?;
    u64::try_from(bytes.len()).is_ok_and(|n| n <= MAX_RECORD).then_some(bytes)
}

/// The next line, without its newline. Bytes that are not UTF-8 become the replacement character and a line over
/// `MAX_LINE` is cut there, with the rest read and dropped: neither ends the feed or grows without bound. `None` at the end.
async fn next_line(r: &mut (impl AsyncBufRead + Unpin)) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut any = false;
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return Ok(any.then(|| String::from_utf8_lossy(&line).into_owned()));
        }
        any = true;
        let (take, ended) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (i, true),
            None => (buf.len(), false),
        };
        let room = MAX_LINE.saturating_sub(line.len());
        line.extend_from_slice(&buf[..take.min(room)]);
        r.consume(take + usize::from(ended));
        if ended {
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}

pub struct CommandLog {
    cmd: Vec<String>,
}

pub fn create_command_log(cmd: &[String]) -> CommandLog {
    CommandLog { cmd: cmd.to_vec() }
}

impl LogFeed for CommandLog {
    fn start(self: Box<Self>) -> Receiver<LogEntry> {
        let (tx, rx) = channel(QUEUE);
        tokio::spawn(async move {
            let Some((prog, args)) = self.cmd.split_first() else { return };
            loop {
                let child =
                    tokio::process::Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn();
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        // not retried: a command that cannot start will not start in five seconds either
                        let _ = tx.send(LogEntry::plain(Level::Warn, format!("log_cmd could not be started ({prog}): {e}"))).await;
                        return;
                    }
                };
                let mut out = BufReader::new(child.stdout.take().expect("piped stdout"));
                let why = loop {
                    match next_line(&mut out).await {
                        Ok(Some(line)) => {
                            if let Some(entry) = wire::log_line(&line)
                                && tx.send(entry).await.is_err()
                            {
                                return;
                            }
                        }
                        Ok(None) => break "log_cmd ended".to_string(),
                        // a command that follows a log never exits by itself, so it is stopped here or the feed stays silent
                        Err(e) => break format!("log_cmd could not be read ({e})"),
                    }
                };
                let _ = child.kill().await;
                if tx.send(LogEntry::plain(Level::Warn, format!("{why}; starting it again in 5 s"))).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        rx
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

/// The records in one directory, read with `parse`, counting toward `seen` across directories.
async fn records(dir: &Path, parse: fn(&serde_json::Value) -> Option<Swap>, seen: &mut usize, out: &mut Vec<Swap>) -> anyhow::Result<()> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let p = e.path();
        // the store keeps .bak and .tmp files beside each record
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        *seen += 1;
        if *seen > MAX_RECORDS {
            bail!("more than {MAX_RECORDS} swap records");
        }
        // a record caught mid-write is skipped and read whole on the next pass
        let bytes = tokio::time::timeout(RECORD_WAIT, record(&p)).await.ok().flatten();
        if let Some(s) = bytes.and_then(|b| serde_json::from_slice(&b).ok()).and_then(|v| parse(&v)) {
            out.push(s);
        }
    }
    Ok(())
}

impl SwapStore for FileSwaps {
    /// Both directions. `reverse/swaps/` is only there once the reverse side has run: its absence is no records, any
    /// other failure to read it a failed read, as for `swaps/`.
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move {
            let (mut out, mut seen) = (Vec::new(), 0usize);
            records(&self.dir.join("swaps"), wire::swap, &mut seen, &mut out).await?;
            match records(&self.dir.join("reverse/swaps"), wire::reverse_swap, &mut seen, &mut out).await {
                Err(e) if e.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) => {}
                other => other?,
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::fast_bridge::model::Direction;

    async fn lines(bytes: &[u8]) -> Vec<String> {
        let mut r = BufReader::with_capacity(16, bytes);
        let mut out = Vec::new();
        while let Some(l) = next_line(&mut r).await.expect("a read") {
            out.push(l);
        }
        out
    }

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

    #[tokio::test]
    async fn bytes_that_are_not_utf8_do_not_end_the_feed() {
        let log = b"first\n\xff\xfe broken\nlast\n";

        let read = lines(log).await;

        assert_eq!(read, ["first", "\u{fffd}\u{fffd} broken", "last"]);
    }

    #[tokio::test]
    async fn a_line_with_no_end_is_cut_and_the_next_one_is_still_read() {
        let mut log = vec![b'a'; MAX_LINE * 3];
        log.extend_from_slice(b"\nnext");

        let read = lines(&log).await;

        assert_eq!((read.len(), read[0].len(), read[1].as_str()), (2, MAX_LINE, "next"));
    }
}
