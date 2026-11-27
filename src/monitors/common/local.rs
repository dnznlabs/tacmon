//! What every monitor reads from this machine alike: a command's output line by line, and a directory of JSON records.

use super::ports::QUEUE;
use anyhow::bail;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::mpsc::{Receiver, channel},
};

/// The most of one log line that is kept. The provider's lines are a few hundred bytes.
pub const MAX_LINE: usize = 64 << 10;
/// The most one record may hold. The provider's are a few kilobytes.
pub const MAX_RECORD: u64 = 1 << 20;
/// The most records read in one pass, counting records only and not the files the store keeps beside each. Past
/// this the read fails: a part of the records would be drawn as all of them.
pub const MAX_RECORDS: usize = 20_000;
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

/// The `.json` records in one directory, read with `parse`, counting toward `seen` across directories. One that
/// `parse` does not take is left out.
pub async fn json_records<T>(dir: &Path, parse: fn(&serde_json::Value) -> Option<T>, seen: &mut usize, out: &mut Vec<T>) -> anyhow::Result<()> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let p = e.path();
        // the store keeps .bak and .tmp files beside each record
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        *seen += 1;
        if *seen > MAX_RECORDS {
            bail!("more than {MAX_RECORDS} records");
        }
        // a record caught mid-write is skipped and read whole on the next pass
        let bytes = tokio::time::timeout(RECORD_WAIT, record(&p)).await.ok().flatten();
        if let Some(s) = bytes.and_then(|b| serde_json::from_slice(&b).ok()).and_then(|v| parse(&v)) {
            out.push(s);
        }
    }
    Ok(())
}

/// How many `.json` records a directory holds and when the newest was last written, in seconds since 1970. Names
/// and times only: no record is opened. A link is not counted: its time is not a record's.
pub async fn newest_record(dir: &Path) -> anyhow::Result<(usize, Option<u64>)> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    let (mut count, mut newest) = (0usize, None);
    while let Some(e) = rd.next_entry().await? {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(meta) = tokio::fs::symlink_metadata(&p).await else { continue };
        if !meta.is_file() {
            continue;
        }
        count += 1;
        if count > MAX_RECORDS {
            bail!("more than {MAX_RECORDS} records");
        }
        let at = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
        newest = newest.max(at);
    }
    Ok((count, newest))
}

/// Whether a failed read of a directory is only that it is not there.
pub fn not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
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

/// Follows a command's output: each line `parse` takes is sent on, and the command is started again five seconds
/// after it ends. `trouble` makes the entry that says the reader itself has something to report. The arguments are
/// passed as a list, never through a shell. Must be called inside the runtime.
pub fn follow<T: Send + 'static>(cmd: Vec<String>, parse: fn(&str) -> Option<T>, trouble: fn(String) -> T) -> Receiver<T> {
    let (tx, rx) = channel(QUEUE);
    tokio::spawn(async move {
        let Some((prog, args)) = cmd.split_first() else { return };
        loop {
            let child =
                tokio::process::Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    // not retried: a command that cannot start will not start in five seconds either
                    let _ = tx.send(trouble(format!("log_cmd could not be started ({prog}): {e}"))).await;
                    return;
                }
            };
            let mut out = BufReader::new(child.stdout.take().expect("piped stdout"));
            let why = loop {
                match next_line(&mut out).await {
                    Ok(Some(line)) => {
                        if let Some(entry) = parse(&line)
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
            if tx.send(trouble(format!("{why}; starting it again in 5 s"))).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn lines(bytes: &[u8]) -> Vec<String> {
        let mut r = BufReader::with_capacity(16, bytes);
        let mut out = Vec::new();
        while let Some(l) = next_line(&mut r).await.expect("a read") {
            out.push(l);
        }
        out
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

    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_is_followed_line_by_line_and_one_that_cannot_start_says_so() {
        let cmd = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let keep = |l: &str| (!l.is_empty()).then(|| l.to_string());

        let mut ran = follow(cmd(&["printf", "one\\n\\ntwo\\n"]), keep, |why| format!("! {why}"));
        let mut missing = follow(cmd(&["/nonexistent/tacmon-test-command"]), keep, |why| format!("! {why}"));

        let read = (ran.recv().await, ran.recv().await, ran.recv().await);
        assert_eq!(read, (Some("one".into()), Some("two".into()), Some("! log_cmd ended; starting it again in 5 s".into())));
        assert!(missing.recv().await.is_some_and(|l| l.starts_with("! log_cmd could not be started")));
    }
}
