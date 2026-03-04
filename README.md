# bank-cluster

A distributed banking proof-of-concept in Rust using **gRPC**, **Raft consensus**, and a **gossip-based membership mesh**, with a resilient client and automated test harness.

---

## 🧱 Milestone 0 — Project scaffolding

### Checklist
- [x] Create Rust workspace with the following crates:
    - `bank_api`
    - `raft_core`
    - `gossip`
    - `node`
    - `client`
    - `harness`
- [x] Add Protobuf files:
    - `proto/bank.proto`
    - `proto/raft.proto`
    - `proto/gossip.proto`
- [x] Set up build scripts for Protobuf codegen
- [x] Configure `Cargo.toml` for tonic/prost
- [x] Ensure project builds (`cargo build`)
- [x] Ensure proto codegen runs (`cargo test`)

**Exit criteria**
- [x] Project compiles cleanly
- [x] Protobuf bindings are generated

---

## 🗃️ Milestone 1 — Single-node WAL + deterministic state machine

### Checklist
- [x] Create WAL module (append + replay)
- [x] Define state machine:
    - accounts map
    - dedupe map (`ClientTxId -> outcome`)
- [ ] Implement gRPC `BankService`
    - `CreateAccount`
    - `GetBalance`
    - `Transfer`
    - `GetTransferStatus`
- [x] Implement idempotent transfer logic
- [ ] Implement WAL persistence and replay on startup

**Exit criteria**
- [ ] WAL replay produces correct balances
- [ ] Idempotency works
- [ ] After restart, state is correct

---

## 🌐 Milestone 2 — Gossip membership mesh

### Checklist
- [ ] Implement gossip service stub (`Exchange(GossipMessage)`)
- [ ] Implement membership view and merging logic
- [ ] Gossip propagation loop
- [ ] Periodic outbound gossip to random peers

**Exit criteria**
- [ ] New node joins via gossip
- [ ] Membership views converge across nodes

---

## 🪩 Milestone 3 — Raft core: election + log replication

### Checklist
- [ ] Implement Raft states: Leader, Follower, Candidate
- [ ] Implement election timeout + randomized timer
- [ ] Implement `RequestVote` RPC and response logic
- [ ] Implement `AppendEntries` for heartbeat
- [ ] Update follower to track `leader_id`

**Exit criteria**
- [ ] Leader election works
- [ ] Leader re-election after failure

---

## 📜 Milestone 4 — Full Raft command replication

### Checklist
- [ ] Integrate bank commands into Raft log entries
- [ ] Leader appends and replicates log entries
- [ ] Follower appends and acknowledges
- [ ] Leader commits after quorum
- [ ] All nodes apply committed entries

**Exit criteria**
- [ ] Commands commit only after majority
- [ ] State converges across nodes

---

## 🧠 Milestone 5 — Follower proxying + preliminary client

### Checklist
- [ ] Make followers proxy `BankService` calls to leader
- [ ] Client library initial implementation
    - Generate stable `ClientTxId`
    - Retry logic with backoff
    - Leader redirect handling
- [ ] Test client calls via any node

**Exit criteria**
- [ ] Client retries reliably
- [ ] Proxying works under leader changes

---

## 🚀 Milestone 6 — Client usability & documentation

### Checklist
- [ ] Define ergonomic Rust client API (`BankClient`)
- [ ] Implement:
    - `transfer`
    - `get_balance`
    - `get_transfer_status`
- [ ] Add tests:
    - Retry behavior
    - Leader failover scenarios
- [ ] Create CLI examples
- [ ] Write documentation + README usage

**Exit criteria**
- [ ] Client is easy to use and documented

---

## 📦 Milestone 7 — Snapshots & log compaction

### Checklist
- [ ] Implement Raft snapshot creation
- [ ] Implement `InstallSnapshot` RPC
- [ ] Add WAL truncation on snapshot
- [ ] Follower catch-up via snapshot

**Exit criteria**
- [ ] Long logs are compacted
- [ ] Slow followers catch up via snapshot

---

## 🔁 Milestone 8 — Dynamic membership with joint consensus

### Checklist
- [ ] Implement joint consensus:
    - `C_old,new` phase
    - Transition to `C_new`
- [ ] Represent membership change entries in Raft log
- [ ] Client updates membership for leader discovery

**Exit criteria**
- [ ] Nodes can be added/removed at runtime
- [ ] Cluster remains available during reconfig

---

## 🧪 Milestone 9 — Fault injection & invariants harness

### Checklist
- [ ] Build fault injection harness
    - Network partitions
    - Delay/skewed messages
    - Node kill/restarts
- [ ] Define invariants:
    - No negative balances
    - Idempotency holds
    - Total money conserved
    - View convergence
- [ ] Automate fault tests in CI

**Exit criteria**
- [ ] Fault tests run reliably and pass

---

## 📌 Optional Extensions

- [ ] Add support for **non-voting learner nodes**
- [ ] Add **read routing policies** (strong vs stale)
- [ ] Add **observability** (metrics, tracing)
- [ ] Add **security** (mTLS, authentication)
- [ ] Clients in other languages

---
