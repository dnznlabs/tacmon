//! What is read from this machine: the provider's log through a command, its swap records from disk.

use super::wire;
use crate::monitors::{
    btc_eth::{
        model::{Level, LogEntry, Swap},
        ports::{Fut, LogFeed, SwapStore},
    },
    common::local::{follow, json_records, not_found},
};
use std::path::PathBuf;
use tokio::sync::mpsc::Receiver;

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

pub struct FileSwaps {
    dir: PathBuf,
}

pub fn create_file_swaps(data_dir: &std::path::Path) -> FileSwaps {
    FileSwaps { dir: data_dir.to_path_buf() }
}

impl SwapStore for FileSwaps {
    /// Both directions. Only these two directories are opened: the provider's key file sits beside them in a
    /// provider that `setup` made, and is never read. Either directory is only there once its side has had a swap:
    /// its absence is no records, any other failure to read it a failed read.
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move {
            let (mut out, mut seen) = (Vec::new(), 0usize);
            for (dir, parse) in [("swaps", wire::swap as fn(&serde_json::Value) -> Option<Swap>), ("reverse/swaps", wire::reverse_swap)] {
                match json_records(&self.dir.join(dir), parse, &mut seen, &mut out).await {
                    Err(e) if not_found(&e) => {}
                    other => other?,
                }
            }
            // neither directory: a data directory that is not there at all is a failed read, not a provider without swaps
            if out.is_empty() && seen == 0 && tokio::fs::metadata(&self.dir).await.is_err() {
                anyhow::bail!("the data directory cannot be read");
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::btc_eth::model::Direction;

    #[tokio::test]
    async fn both_directions_are_read_a_new_provider_has_no_records_and_a_missing_directory_is_a_failed_read() {
        let dir = std::env::temp_dir().join(format!("tacmon-test-btc-eth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        // the key file a set-up provider keeps here: never a record, whatever is in it
        std::fs::write(dir.join("provider.key"), "00".repeat(32)).expect("a file");
        let fresh = create_file_swaps(&dir).swaps().await.expect("a read").len();
        std::fs::create_dir_all(dir.join("swaps")).expect("a directory");
        std::fs::create_dir_all(dir.join("reverse/swaps")).expect("a directory");
        std::fs::write(dir.join("swaps/f.json"), r#"{"id":"fw","phase":"done","offer":{"btc":{"value":"100000"},"evm":{"amount":"3100000"}}}"#)
            .expect("a record");
        std::fs::write(dir.join("reverse/swaps/r.json"), r#"{"id":"rv","phase":"built","amount":"100000","evmAmount":"3120000"}"#).expect("a record");

        let mut read = create_file_swaps(&dir).swaps().await.expect("a read");
        let missing = create_file_swaps(&dir.join("not-there")).swaps().await;

        let _ = std::fs::remove_dir_all(&dir);
        read.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(fresh, 0);
        assert_eq!(
            read.iter().map(|s| (s.id.as_str(), s.direction, s.sats, s.eth)).collect::<Vec<_>>(),
            [("fw", Direction::Forward, 100_000, 3_100_000), ("rv", Direction::Reverse, 100_000, 3_120_000)]
        );
        assert!(missing.is_err());
    }
}
