# Integration — Feature Flags, Cargo.toml, and Crate Integrations

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `async` | ON | tokio async API (RPITIT trait methods) |
| `store-lance` | ON | LanceDB (FactStore) + LanceGraph (RelationshipStore) |
| `store-duck` | ON | DuckDB (LifecycleStore) |
| `embedding-local` | OFF | mistral.rs local inference (heavy dependency) |
| `pyo3-bindings` | OFF | Python bindings for arktrace (Phase 3) |
| `audit-bridge` | OFF | edgesentry-audit integration (Phase 3) |

---

## Cargo.toml

```toml
[package]
name = "membrane"
version = "0.1.0"
edition = "2021"

[features]
default = ["async", "store-lance", "store-duck"]
async = ["dep:tokio"]
embedding-local = ["dep:mistralrs", "dep:mistralrs-core"]
store-lance = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema", "dep:arrow-cast"]
store-duck = ["dep:duckdb"]
pyo3-bindings = ["dep:pyo3"]
audit-bridge = ["dep:edgesentry-audit"]

[dependencies]
# Align with edgesentry-rs workspace versions
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
blake3 = "1.5"
postcard = { version = "1.1", default-features = false, features = ["alloc"] }
tracing = "0.1"
hex = "0.4"
uuid = { version = "1", features = ["v4"] }

# Arrow is always present — it is the IO common language
arrow-array  = { version = "53" }
arrow-schema = { version = "53" }

tokio = { version = "1", features = ["rt", "sync", "macros", "time"], optional = true }

lancedb    = { version = "0.9",  optional = true }
arrow-cast = { version = "53",   optional = true }   # for Lance ↔ DuckDB Arrow IPC

duckdb = { version = "1.1", features = ["bundled"], optional = true }

mistralrs      = { version = "0.3", optional = true }
mistralrs-core = { version = "0.3", optional = true }

pyo3 = { version = "0.22", features = ["extension-module"], optional = true }

edgesentry-audit = { path = "../edgesentry-rs/crates/edgesentry-audit", optional = true }

[dev-dependencies]
tokio    = { version = "1", features = ["rt-multi-thread", "macros"] }
tempfile = "3"
```

Note: `arrow-array` and `arrow-schema` are **non-optional** — Arrow is the IO boundary for all backends.

---

## Integration with Existing Crates

### edgesentry-audit

`AuditBridge` (`audit-bridge` feature): each `store_episode()` call emits a blake3-signed `AuditRecord` via the existing `AuditLedger` trait. Memory writes become tamper-evident.

```rust
// audit/ledger.rs
pub struct AuditBridge<L: AuditLedger> { ledger: L }
impl<L: AuditLedger> AuditBridge<L> {
    pub fn record_store(&mut self, id: &MemoryId, content_hash: &Hash32) -> Result<()>;
}
```

### arktrace (Python)

Phase 3: PyO3 bindings expose `MembraneSession` as a Python class:

```python
import membrane
session = membrane.MembraneSession(lance_uri="./data/memory/facts.lance",
                                   duck_path="./data/memory/lifecycle.db")
await session.store_episode(role="user", content="...")
ctx = await session.assemble_context(query="...", token_budget=4096)
```

arktrace's existing LanceDB files at `data/processed/` share the same Lance format — schema alignment is a configuration concern, not a code change.

### edgesentry-inspect

Future: `ContextAssembler` prepends relevant past findings before processing a `SensorFrame`, enabling inspection reports that "remember" prior observations about a vessel or location.
