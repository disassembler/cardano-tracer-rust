# Testing hermod

## Integration Test with hermod-tracer

### Requirements

- **hermod-tracer**: Built from [hermod-tracing](https://github.com/input-output-hk/hermod-tracing)
- **hermod** (this library): `nix build` or `nix develop --command cargo build`

### Quick Start

```bash
# 1. Build and start hermod-tracer
git clone https://github.com/input-output-hk/hermod-tracing
cd hermod-tracing
nix build .\#hermod-tracer
./result/bin/hermod-tracer --config /tmp/tracer-test-config.yaml

# 2. In another terminal, run the integration example
cd path/to/hermod
RUST_LOG=info nix develop --command cargo run --example mux_test

# 3. Check output
ls /tmp/hermod-tracer-test-logs/
```

### hermod-tracer Config

File: `/tmp/tracer-test-config.yaml`
```yaml
network:
  tag: AcceptAt
  contents: /tmp/hermod-tracer.sock
networkMagic: 764824073
logging:
  - logRoot: /tmp/hermod-tracer-test-logs
    logMode: FileMode
    logFormat: ForHuman
loRequestNum: 100
```

### Unit Tests

```bash
nix develop --command cargo test
```

All 9 tests should pass, including CBOR round-trip tests for `TraceObject`, `Severity`, and `DetailLevel`.

---

## Protocol Compatibility

### Verified Wire Format

The following have been verified to match the Haskell implementation byte-for-byte:

- **TraceObject**: `array(9)[0, toHuman, toMachine, toNamespace, toSeverity, toDetails, toTimestamp, toHostname, toThreadId]`
  - Constructor index `0` prefix required (Haskell Generic Serialise)
- **Severity**: `array(1)[constructor_index]` — e.g., `Info` → `[1]`
- **DetailLevel**: `array(1)[constructor_index]` — e.g., `DNormal` → `[1]`
- **Maybe/Option**: `Nothing` → `[]`, `Just x` → `[x]`
- **UTCTime**: CBOR tag 1 + float64 (seconds since Unix epoch)
- **MsgTraceObjectsRequest**: `array(3)[1, bool, array(2)[0, count]]`
- **MsgTraceObjectsReply**: `array(2)[3, array(N)[...]]`
- **MsgDone**: `array(1)[2]`

### Haskell Generic Serialise Rules

Types using `deriving anyclass (Serialise)` via GHC Generics encode as:

| Type | Encoding |
|------|----------|
| Product with N fields | `array(N+1)[constructor_index, field1, ..., fieldN]` |
| Nullary constructor (enum) | `array(1)[constructor_index]` |
| Newtype | `array(2)[0, value]` |

---

## Protocol Numbers (Confirmed)

| Protocol | Number |
|----------|--------|
| Handshake | 0 |
| EKG | 1 |
| TraceObject | 2 |
| DataPoint | 3 |

The EKG (32769 / 0x8001) and DataPoint (32771 / 0x8003) warnings from Pallas are expected — these are the initiator-flagged versions of those protocol IDs and are not yet implemented.
