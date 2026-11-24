//! What is read from this machine: the provider's log through a command, its swap records from disk.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Level, LogEntry, Swap},
    ports::{Fut, LogFeed, SwapStore},
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    sync::mpsc::{UnboundedReceiver, unbounded_channel},
};

/// The most of one log line that is kept. The provider's lines are a few hundred bytes.
const MAX_LINE: usize = 64 << 10;
/// The most one swap record may hold. The provider's are a few kilobytes.
const MAX_RECORD: u64 = 1 << 20;

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
    fn start(self: Box<Self>) -> UnboundedReceiver<LogEntry> {
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            let Some((prog, args)) = self.cmd.split_first() else { return };
            loop {
                let child =
                    tokio::process::Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn();
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        // not retried: a command that cannot start will not start in five seconds either
                        let _ = tx.send(LogEntry::plain(Level::Warn, format!("log_cmd could not be started ({prog}): {e}")));
                        return;
                    }
                };
                let mut out = BufReader::new(child.stdout.take().expect("piped stdout"));
                let why = loop {
                    match next_line(&mut out).await {
                        Ok(Some(line)) => {
                            if let Some(entry) = wire::log_line(&line)
                                && tx.send(entry).is_err()
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
                if tx.send(LogEntry::plain(Level::Warn, format!("{why}; starting it again in 5 s"))).is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        rx
    }
}

pub struct FileSwaps {
    dir: PathBuf,
}

pub fn create_file_swaps(data_dir: &std::path::Path) -> FileSwaps {
    FileSwaps { dir: data_dir.join("swaps") }
}

impl SwapStore for FileSwaps {
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move {
            let mut rd = tokio::fs::read_dir(&self.dir).await?;
            let mut out = Vec::new();
            while let Some(e) = rd.next_entry().await? {
                let p = e.path();
                // the store keeps .bak and .tmp files beside each record
                if p.extension().is_none_or(|x| x != "json") {
                    continue;
                }
                // only an ordinary file of a record's size: a pipe would never end and a link to a device never stops
                if !tokio::fs::metadata(&p).await.is_ok_and(|m| m.is_file() && m.len() <= MAX_RECORD) {
                    continue;
                }
                // a record caught mid-write is skipped and read whole on the next pass
                if let Some(s) = tokio::fs::read(&p).await.ok().and_then(|b| serde_json::from_slice(&b).ok()).and_then(|v| wire::swap(&v)) {
                    out.push(s);
                }
            }
            Ok(out)
        })
    }
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
}
