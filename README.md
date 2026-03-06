# bank-cluster

A distributed banking proof-of-concept in Rust using **gRPC**, **Raft consensus**, and a **gossip-based membership mesh**, with a resilient client and automated test harness.

---

## Getting started

### Prerequisites

| Tool | Install |
|---|---|
| Rust (stable) | [rustup.rs](https://rustup.rs) |
| protoc | `brew install protobuf` / `apt-get install protobuf-compiler` |
| bacon | `make install-tools` |
| cargo-nextest | `make install-tools` |

### After cloning

```bash
make install        # installs bacon, cargo-nextest, verifies protoc, and wires the pre-commit hook
```

### Running the node

```bash
cargo run --bin node          # starts the gRPC server on [::1]:50051
```

Or with hot-reload on every file save:

```bash
bacon                         # default job: check-all
# press 'r' to switch to run-node (kill_then_restart on change)
```

---

## Project structure

```
bank-cluster/
├── proto/                    # Protobuf schemas (bank, raft, gossip)
├── bank_api/                 # Prost/tonic generated gRPC bindings
├── node/                     # Single-node server binary
│   └── src/
│       ├── main.rs           # Wires Store + BankGrpcService + tonic Server
│       ├── service/          # gRPC handler impl + service integration tests
│       ├── store/            # Store: WAL + StateMachine coordinator
│       ├── state_machine/    # Pure in-memory state machine + unit tests
│       └── wal/              # Append-only write-ahead log
├── client/                   # Client library (in progress)
├── raft_core/                # Raft consensus core (in progress)
├── gossip/                   # Gossip membership (in progress)
├── harness/                  # Fault-injection test harness (in progress)
├── http/                     # HTTP client smoke-test files (JetBrains)
├── scripts/
│   ├── hooks/pre-commit      # Checked-in pre-commit hook source
│   └── install-hooks.sh      # Copies hooks into .git/hooks/
├── .github/
│   ├── workflows/ci.yml      # GitHub Actions CI pipeline
│   └── dependabot.yml        # Automated dependency updates
├── .config/nextest.toml      # nextest profiles (default + ci)
├── bacon.toml                # bacon job definitions
└── Makefile                  # Developer setup entrypoint
```

---

## Architecture — node internals

```
gRPC request
     │
     ▼
 service.rs          ← decodes proto, maps domain results to proto responses
     │
     ▼
 Store::execute      ← assembles LogEntry, writes WAL, applies to state machine
     │
     ├──▶ Wal::append        ← appends encoded entry to disk, fsync
     │
     └──▶ StateMachine::apply ← mutates in-memory accounts + dedupe maps
```

On startup `Store::open` replays all WAL entries through the state machine to restore state.

### Key design decisions

- **Proto boundary** — `BankCommand` encodes/decodes via prost at the WAL boundary. Proto types never leak into the state machine; `TransferResult` is a domain enum mapped to `TransferStatus` only in `service.rs`.
- **Idempotency** — every transfer carries a `client_tx_id`; the outcome is cached in the state machine's dedupe map and replayed from the WAL across restarts.
- **WAL ordering** — `StateMachine::apply` asserts `index == last_applied_index + 1`, catching any gap or replay disorder.
- **Separate progress cursors** — `Wal::last_index` (durably stored) and `StateMachine::last_applied_index` (executed) are tracked independently to support future snapshotting.

---

## gRPC API

Defined in `proto/bank.proto`, served on `[::1]:50051`.

| RPC | Request | Response |
|---|---|---|
| `CreateAccount` | `account_id`, `initial_balance` | `success`, `message` |
| `GetBalance` | `account_id` | `balance` |
| `Transfer` | `from`, `to`, `amount`, `client_tx_id` | `TransferStatus` |
| `GetTransferStatus` | `client_tx_id` | `TransferStatus` |

`TransferStatus` values: `COMMITTED_OK`, `COMMITTED_INSUFFICIENT_FUNDS`, `COMMITTED_INVALID_ACCOUNT`.

### Smoke testing with HTTP files

JetBrains HTTP client files are in `http/`. Run against a live node:

```
http/create_account.http       # create accounts, duplicate handling
http/get_balance.http          # balance reads, not-found case
http/transfer.http             # ok, idempotent retry, insufficient funds, invalid account
http/get_transfer_status.http  # cached status, unknown tx
http/e2e_smoke_test.http       # full 11-step lifecycle in order
```

Set the target in `http/http-client.env.json` (default: `localhost:50051`).

---

## Testing

```bash
cargo nextest run --all                   # all tests, default profile
cargo nextest run -p node                 # node crate only
cargo nextest run --all --profile ci      # CI profile: no retries, JUnit output
```

### Test layers

| Layer | Location | What it covers |
|---|---|---|
| WAL unit tests | `wal/wal.rs`, `wal/entry.rs` | encode/decode, replay, persistence, edge cases |
| State machine unit tests | `state_machine/mod.rs` | apply, idempotency, restore from entries |
| Store integration tests | `store/integration_tests.rs` | full WAL + state machine via `Store`, crash recovery |
| gRPC integration tests | `service/integration_tests.rs` | in-process tonic server, all four endpoints |

---

## Developer tooling

### bacon — watch mode

```bash
bacon                   # check-all on every save (default)
```

| Key | Job |
|---|---|
| `f` | `fmt` — auto-format |
| `b` | `build` — full build |
| `c` | `clippy` — lint |
| `p` | `pedantic` — pedantic clippy |
| `t` | `test` — all tests via nextest |
| `n` | `test-node` — node crate only |
| `r` | `run-node` — hot-reload server |
| `d` | `doc` — build docs |

### Pre-commit hook

Installed by `make install`. Runs on every `git commit`:

1. `cargo fmt --all -- --check`
2. `cargo build --all-targets`
3. `cargo check --all-targets`
4. `cargo clippy --all-targets -- -D warnings`
5. `cargo nextest run --all`

To skip in an emergency: `git commit --no-verify`.

### Makefile

```bash
make install            # install all tools + git hooks (run after cloning)
make install-tools      # bacon, cargo-nextest, verify protoc
make install-hooks      # copy scripts/hooks/ → .git/hooks/
```

---

## CI — GitHub Actions

Pipeline defined in `.github/workflows/ci.yml`. Runs on every push and pull request to any branch.

| Job | Depends on | What it runs |
|---|---|---|
| `fmt` | — | `cargo fmt --check` |
| `build` | — | `cargo build --all-targets` |
| `check` | `build` | `cargo check --all-targets` |
| `clippy` | `build` | `cargo clippy -- -D warnings` |
| `test` | `build` | `cargo nextest run --all --profile ci` |

All jobs that compile code install `protoc` via `arduino/setup-protoc`. Build artifacts are cached by `Cargo.lock` hash to speed up downstream jobs.

Dependency updates are managed by **Dependabot** (`.github/dependabot.yml`), scanning both Cargo and GitHub Actions ecosystems weekly.

---

## Milestones

### ✅ Milestone 0 — Project scaffolding
- [x] Rust workspace: `bank_api`, `raft_core`, `gossip`, `node`, `client`, `harness`
- [x] Protobuf files: `bank.proto`, `raft.proto`, `gossip.proto`
- [x] Build scripts for prost/tonic codegen
- [x] Project compiles cleanly

### ✅ Milestone 1 — Single-node WAL + state machine
- [x] WAL module: append, replay, fsync, sequential index validation
- [x] State machine: accounts map, dedupe map, `last_applied_index`
- [x] `Store` coordinator: `open` (replay on startup), `execute` (WAL + apply)
- [x] gRPC `BankService`: `CreateAccount`, `GetBalance`, `Transfer`, `GetTransferStatus`
- [x] Idempotent transfer logic with `client_tx_id`
- [x] WAL persistence and replay on startup
- [x] Proto boundary enforced — domain types only inside state machine
- [x] Comprehensive test suite (WAL, state machine, store, gRPC)
- [x] HTTP smoke-test files for manual API testing
- [x] bacon watch mode with hot-reload
- [x] nextest configured with `default` and `ci` profiles
- [x] Pre-commit hook: fmt → build → check → clippy → test
- [x] GitHub Actions CI pipeline with protoc support
- [x] Dependabot for Cargo + Actions

### 🔲 Milestone 2 — Gossip membership mesh
- [ ] Gossip service stub (`Exchange(GossipMessage)`)
- [ ] Membership view and merging logic
- [ ] Gossip propagation loop

### 🔲 Milestone 3 — Raft core: election + log replication
- [ ] Raft states: Leader, Follower, Candidate
- [ ] Election timeout + randomized timer
- [ ] `RequestVote` RPC
- [ ] `AppendEntries` for heartbeat

### 🔲 Milestone 4 — Full Raft command replication
- [ ] Integrate bank commands into Raft log entries
- [ ] Leader appends and replicates; follower acknowledges
- [ ] Leader commits after quorum; all nodes apply

### 🔲 Milestone 5 — Follower proxying + client
- [ ] Followers proxy `BankService` calls to leader
- [ ] Client library: `ClientTxId` generation, retry + backoff, leader redirect

### 🔲 Milestone 6 — Client usability & documentation
- [ ] Ergonomic `BankClient` Rust API
- [ ] CLI examples

### 🔲 Milestone 7 — Snapshots & log compaction
- [ ] Raft snapshot creation + `InstallSnapshot` RPC
- [ ] WAL truncation; slow-follower catch-up via snapshot

### 🔲 Milestone 8 — Dynamic membership (joint consensus)
- [ ] Joint consensus: `C_old,new` → `C_new`
- [ ] Membership change entries in Raft log

### 🔲 Milestone 9 — Fault injection & invariants harness
- [ ] Network partitions, delays, node kill/restart
- [ ] Invariants: no negative balances, idempotency, money conservation
- [ ] Automated fault tests in CI

---

## Optional extensions

- [ ] Non-voting learner nodes
- [ ] Read routing policies (strong vs stale reads)
- [ ] Observability (metrics, tracing)
- [ ] Security (mTLS, authentication)
- [ ] Clients in other languages

