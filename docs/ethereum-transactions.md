# What tacmon can show of the Ethereum side

Written for [#6](https://github.com/dnznlabs/tacmon/issues/6), which asked whether the provider's Ethereum
transactions can be listed from the configured RPC alone, with no indexer and no change to the provider.
Checked against the mainnet pool in November 2026.

## The short answer

One of them can: the lock a forward swap puts into the pool. Nothing else.

## What was checked

A forward swap has three Ethereum transactions: the provider's lock of pool TAC for the bridger, the bridger's
claim of it, and the provider's refund if the deadline passes. The provider's own account also sends a few of its
own: a pull from a treasury, a deposit into the pool, a settle when the relay is not sending them.

| | findable from the RPC | why |
|---|---|---|
| pool lock (forward) | yes | the pool emits `LockLeavesInserted(uint256 indexed firstLockIndex, bytes32[] lockLeaves)`, and the swap record carries the same leaf as `lock.lockLeaf` |
| claim, refund | no | they spend a lock nullifier, which the pool writes to storage and emits nothing for. The provider finds them by reading the calldata of every pool transaction in the window |
| the account's own sends | no | an RPC has no list of an account's transactions. Token transfers are indexed by account, but they are a part of it, and the swap transactions are not the account's at all |
| a transaction that reverted | no | it leaves no log |
| pool lock (reverse) | no | the lock is the bridger's, and the provider's record does not carry its leaf |

A relay sends the lock, the claim and the refund. None of them is at the provider's account, so nothing indexed by
account leads to them, and none of them is the operator's to speed up.

## How far back a public node serves the log

`eth_getLogs` for one contract and one topic, on free endpoints with no key:

| node | allowed |
|---|---|
| publicnode | about 10,000 blocks (33 hours); a wider range needs a token |
| drpc | 100 blocks |
| 1rpc | 50 blocks |
| ankr, cloudflare, llamarpc | a key, a refusal, or no answer |

A refusal comes back as a JSON-RPC error with a 200, a 400 or a 403 depending on the node. A day of the whole
pool's lock events is under a hundred entries and some 20 KiB.

So tacmon reads a day back, halves that each time a node refuses, and on a narrow node has only what it saw
while it was running.

## What it would take to show the rest

- **An indexer** (an Etherscan-style API) lists the account's own sends. It needs a key and tells one more party
  which account is watched, so it would be an opt-in setting.
- **The provider recording the hashes it sends** in its swap records would make every one of them a lookup by
  hash, reverted ones included. That is a change to tacit, not to tacmon.
