//! How a monitor's readers hand what they read to its state.

use crate::shell::monitor::Wake;
use tokio::sync::mpsc::Sender;

/// Where a reader sends its events: the monitor's queue, and the shell to come and empty it.
pub struct Out<E> {
    pub tx: Sender<E>,
    pub wake: Wake,
}

// by hand: a derive would ask that the event be `Clone`, which nothing here needs
impl<E> Clone for Out<E> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), wake: self.wake.clone() }
    }
}

impl<E> Out<E> {
    /// `false` once the monitor has gone, which ends the task that asked. Waits while the queue is full.
    pub async fn send(&self, ev: E) -> bool {
        // woken first as well: a reader waiting on a full queue needs the shell to come and empty it
        self.wake.wake();
        let ok = self.tx.send(ev).await.is_ok();
        self.wake.wake();
        ok
    }
}
