//! The pool contract's log of locks on Ethereum, read a round at a time from a node that may refuse a long range.
//! The log says a lock was made, not whose it is: a monitor tells that from the leaf its swap record names.

use super::{model::PoolScan, ports::PoolLog};

/// How far back the pool's log on Ethereum is read, in blocks: a day. A swap is open for hours.
pub const POOL_LOOKBACK: u64 = 7_200;
/// Blocks after which Ethereum does not take a transaction back: two epochs.
pub const POOL_SETTLED: u64 = 64;
/// Ethereum makes a block every twelve seconds, or skips one: never sooner.
pub const BLOCK_SECS: u64 = 12;

/// The shortest reach back the first read of the pool's log is cut to when a node refuses a longer one. Free public
/// nodes allow from 50 blocks to a day and a half.
const MIN_REACH: u64 = 32;
/// How many of the newest blocks are read again each round: one taken back by a reorganisation takes its locks with it.
const REREAD: u64 = 12;
/// The most locks kept. A day of the whole pool's is under a hundred.
const MAX_POOL_LOCKS: usize = 4096;

/// What has been read of the pool's log, and where the next read starts.
pub struct PoolReader {
    scan: PoolScan,
    /// The block to read from next. `None`: nothing read yet.
    next: Option<u64>,
    /// How far back a first read reaches: halved each time a node refuses it, and never by a request that got no answer.
    reach: u64,
}

impl Default for PoolReader {
    fn default() -> Self {
        Self { scan: PoolScan::default(), next: None, reach: POOL_LOOKBACK }
    }
}

impl PoolReader {
    /// One round: the tip, then the log from where the last read ended. `None` when the node did not answer.
    pub async fn read(&mut self, pool: &dyn PoolLog) -> Option<PoolScan> {
        let tip = pool.tip().await.ok()?;
        // never read, or away for longer than is kept: start over from the tip
        let fresh = self.next.is_none_or(|n| tip.saturating_sub(n) > POOL_LOOKBACK);
        let from = if fresh { tip.saturating_sub(self.reach) } else { self.next.unwrap_or(tip).saturating_sub(REREAD).max(self.scan.since).min(tip) };
        let found = match pool.locks(from, tip).await {
            Ok(Some(found)) => found,
            // refused: a first read reaches half as far next time. One from where the last ended was refused for the
            // blocks that have piled up since, and they only pile higher: it starts over, at a reach the node allowed
            Ok(None) => {
                if fresh {
                    self.reach = (self.reach / 2).max(MIN_REACH);
                }
                self.next = None;
                return None;
            }
            // no answer says nothing about what the node allows
            Err(_) => return None,
        };
        let scan = &mut self.scan;
        if fresh {
            *scan = PoolScan { since: from, ..Default::default() };
        }
        scan.since = scan.since.max(tip.saturating_sub(POOL_LOOKBACK));
        // what was read again replaces what was held for those blocks
        scan.locks.retain(|l| l.block < from && l.block >= scan.since);
        scan.locks.extend(found.into_iter().filter(|l| (from..=tip).contains(&l.block)));
        if let Some(over) = scan.locks.len().checked_sub(MAX_POOL_LOCKS).filter(|n| *n > 0) {
            scan.locks.drain(..over);
            scan.since = scan.locks.first().map_or(tip, |l| l.block + 1);
        }
        scan.tip = tip;
        self.next = Some(tip + 1);
        Some(scan.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::common::{model::PoolLock, ports::Fut};

    /// A node that refuses a read of more than `most` blocks, does not answer while `down`, and notes every range it was asked for.
    struct Narrow {
        tip: std::sync::atomic::AtomicU64,
        most: u64,
        down: std::sync::atomic::AtomicBool,
        asked: std::sync::Mutex<Vec<(u64, u64)>>,
    }

    fn narrow(tip: u64, most: u64) -> Narrow {
        Narrow { tip: tip.into(), most, down: false.into(), asked: Default::default() }
    }

    impl PoolLog for Narrow {
        fn tip(&self) -> Fut<'_, u64> {
            Box::pin(async move { Ok(self.tip.load(std::sync::atomic::Ordering::Relaxed)) })
        }
        fn locks(&self, from: u64, to: u64) -> Fut<'_, Option<Vec<PoolLock>>> {
            Box::pin(async move {
                self.asked.lock().expect("the list").push((from, to));
                anyhow::ensure!(!self.down.load(std::sync::atomic::Ordering::Relaxed), "no answer");
                if to - from > self.most {
                    return Ok(None);
                }
                // one lock in the range, and one outside it that an honest node would not send
                Ok(Some(vec![
                    PoolLock { leaf: "a".repeat(64), block: to, ..Default::default() },
                    PoolLock { leaf: "b".repeat(64), block: from - 1, ..Default::default() },
                ]))
            })
        }
    }

    async fn rounds(reader: &mut PoolReader, node: &Narrow, n: usize) -> Vec<Option<(u64, usize)>> {
        let mut reads = Vec::new();
        for _ in 0..n {
            reads.push(reader.read(node).await.map(|p| (p.since, p.locks.len())));
        }
        reads
    }

    #[tokio::test]
    async fn a_node_that_refuses_a_days_log_is_asked_for_half_as_much_each_round_and_then_only_for_the_newest_blocks() {
        let node = narrow(100_000, 2_000);
        let mut reader = PoolReader::default();

        let reads = rounds(&mut reader, &node, 4).await;

        // a day, half, a quarter (which the node allows), then the newest blocks again: the lock read twice is held once
        assert_eq!(*node.asked.lock().expect("the list"), [(92_800, 100_000), (96_400, 100_000), (98_200, 100_000), (99_989, 100_000)]);
        assert_eq!(reads, [None, None, Some((98_200, 1)), Some((98_200, 1))]);
    }

    #[tokio::test]
    async fn after_a_gap_wider_than_the_node_allows_the_read_starts_over_and_does_not_ask_for_the_same_blocks_for_a_day() {
        use std::sync::atomic::Ordering::Relaxed;
        // allows 50 blocks: the first read gets down to a reach of 32
        let node = narrow(100_000, 50);
        let mut reader = PoolReader::default();
        let _ = rounds(&mut reader, &node, 9).await;
        node.asked.lock().expect("the list").clear();

        // twenty minutes with the lid shut
        node.tip.store(100_100, Relaxed);
        let reads = rounds(&mut reader, &node, 3).await;

        assert_eq!(*node.asked.lock().expect("the list"), [(99_989, 100_100), (100_068, 100_100), (100_089, 100_100)]);
        assert_eq!(reads, [None, Some((100_068, 1)), Some((100_068, 1))]);
    }

    #[tokio::test]
    async fn a_node_that_does_not_answer_costs_no_reach() {
        use std::sync::atomic::Ordering::Relaxed;
        let node = narrow(100_000, 10_000);
        let mut reader = PoolReader::default();
        node.down.store(true, Relaxed);
        let failed = rounds(&mut reader, &node, 8).await;

        node.down.store(false, Relaxed);
        let reads = rounds(&mut reader, &node, 1).await;

        assert_eq!((failed, reads), (vec![None; 8], vec![Some((92_800, 1))]));
    }
}
