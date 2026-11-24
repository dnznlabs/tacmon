//! What is read from this machine: the provider's log through a command, its swap records from disk.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Level, LogEntry, Swap},
    ports::{Fut, LogFeed, SwapStore},
};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc::{UnboundedReceiver, unbounded_channel},
};

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
                let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(entry) = wire::log_line(&line)
                        && tx.send(entry).is_err()
                    {
                        return;
                    }
                }
                let _ = child.wait().await;
                if tx.send(LogEntry::plain(Level::Warn, "log_cmd ended; starting it again in 5 s")).is_err() {
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
                // a record caught mid-write is skipped and read whole on the next pass
                if let Some(s) = tokio::fs::read(&p).await.ok().and_then(|b| serde_json::from_slice(&b).ok()).and_then(|v| wire::swap(&v)) {
                    out.push(s);
                }
            }
            Ok(out)
        })
    }
}
