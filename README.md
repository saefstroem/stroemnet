# stroemnet

A trustless cross-chain atomic swap protocol.

## Roles

Stroemnet has two kinds of participant, split by where they run:

- **Native daemon (`stroemnetd`)**: runs as an **LP** (liquidity provider, answers
  quote requests and settles swaps) or as an **observer / CCR relay** (tracks and
  helps settle swaps without quoting). Built from `crates/node`.
- **WebAssembly SDK (`StroemGateway`)**: the **taker**. Compiled from `crates/wasm`
  and consumed from TypeScript/JavaScript in the browser. The taker requests quotes
  and submits commitments.

### Supported channels

| TOML key / SDK name | `ChannelId` byte | Token | Kind |
|---------------------|:----------------:|-------|------|
| `kaspa-tn10`        | `0`              | KAS   | Kaspa (UTXO) |
| `ethereum-sepolia`  | `1`              | ETH   | EVM  |
| `igra-galleon`      | `2`              | iKAS  | EVM  |

---

## Running the daemon (`stroemnetd`)

The daemon reads **all** of its configuration from a single TOML file (globals,
per-channel settings, and private keys). The file path is the first positional
argument and defaults to `stroemnet.toml` in the working directory.

```sh
# Debug
cargo run -p stroemnet-node --bin stroemnetd -- ./stroemnet.toml

# Release
cargo build --release -p stroemnet-node --bin stroemnetd
./target/release/stroemnetd ./stroemnet.toml   # arg optional -> defaults to ./stroemnet.toml
```

> The config file holds private keys. Protect it with file permissions
> (e.g. `chmod 600 stroemnet.toml`).

### LP node

An LP answers incoming quote requests and signs proposals. Set `lp = true` and
provide a `private_key` for **every** configured channel (the daemon refuses to
start otherwise).

```toml
lp = true

[channels.ethereum-sepolia]
private_key = "0x<sepolia-key>"
rpc_url = "https://eth-sepolia.api.onfinality.io/public"
htlc_address = "0x3AB5f1089f521D982ad67193E8523eB2fD34Da53"

[channels.kaspa-tn10]
private_key = "<kaspa-key-hex>"
network_id = "testnet-10"
```

### Observer

An observer does **not** answer quote requests. It tracks swaps on the network
and relays peer-to-peer messages (reveals, script announcements). Set `lp = false`
or simply omit `lp` (the default).

Competitive Claim Rescue (**CCR**) is configured **per channel** with `participate_ccr`
(default `false`):

- **Observer with CCR** (`participate_ccr = true`) — in addition to tracking, the
  node helps *settle* swaps it observes by submitting claim and refund transactions
  on-chain. Actually broadcasting those transactions requires a funded
  `private_key` for that channel; without a key the node still tracks but cannot
  submit.
- **Observer without CCR** (`participate_ccr = false`) — purely passive. The node
  observes and relays but never submits any chain transactions for that channel.

```toml
# Observer that actively settles (CCR on) on EVM, passive on Kaspa.
lp = false

[channels.ethereum-sepolia]
participate_ccr = true
private_key = "0x<sepolia-key>"   # needed to broadcast settlement txs
rpc_url = "https://eth-sepolia.api.onfinality.io/public"
htlc_address = "0x3AB5f1089f521D982ad67193E8523eB2fD34Da53"

[channels.kaspa-tn10]
participate_ccr = false
network_id = "testnet-10"
```

---

## Configuration file (`stroemnet.toml`)

Full annotated example with every key. Required globals have no default; a missing
one is a clear parse error. Channel tables: the table being present enables that
channel.

```toml
# Globals 
bind_addr = "0.0.0.0:9000" # IPv4 socket to listen on (required)
external_hostname = "wss://your-host.example/"  # how peers reach you (required)
min_trade_usd = 1.0 # reject trades below this (LP only; required when lp = true)
max_trade_usd = 100000.0 # reject trades above this (LP only; required when lp = true)
spread_percent = 0.5 # LP spread, in percent (LP only; required when lp = true)
price_oracle_update_interval_secs = 60 # price refresh cadence (required)

commit_buffer_secs = 960 # optional, default 960 — propagation buffer. added when computing user unlock timestamps
bootstrap_peers = ["wss://a-known-node.example/"]  #optional, default []
lp = false # optional, default false (observer)
peer_db = "./stroemnet-peers.db" # optional, default "./stroemnet-peers.db"

[channels.kaspa-tn10]
private_key = "<kaspa-key-hex>" # required when lp = true; optional for observers
participate_ccr = true # optional, default false
network_id = "testnet-10" # required for Kaspa channels
wrpc_url = "wss://<kaspa-wrpc-node>" # optional — omit to use a public resolver
coinbase_maturity = 100 # optional
script_ttl_secs = 14400 # optional — how long monitored redeem scripts live
min_confirmations = 30 # optional — finality threshold

[channels.ethereum-sepolia]
private_key = "0x<sepolia-key>" # required when lp = true; optional for observers
participate_ccr = true # optional, default false
rpc_url = "https://eth-sepolia.api.onfinality.io/public" # required for EVM channels
htlc_address = "0x3AB5f1089f521D982ad67193E8523eB2fD34Da53" # required for EVM channels
min_confirmations = 1 # optional — finality threshold

[channels.igra-galleon]
private_key = "0x<igra-key>"
participate_ccr = true
rpc_url = "https://galleon-testnet.igralabs.com:8545"
htlc_address = "0x<igra-htlc-address>"
min_confirmations = 1
```

Per-channel field applicability:

| Field | Kaspa | EVM | Notes |
|-------|:-----:|:---:|-------|
| `private_key`       | x | x | required if `lp = true`; needed to submit CCR settlement txs |
| `participate_ccr`   | x | x | default `false` |
| `min_confirmations` | x | x | finality threshold |
| `network_id`        | x |   | **required** for Kaspa |
| `wrpc_url`          | x |   | optional |
| `coinbase_maturity` | x |   | optional |
| `script_ttl_secs`   | x |   | optional |
| `rpc_url`           |   | x | **required** for EVM |
| `htlc_address`      |   | x | **required** for EVM |

---

## WebAssembly SDK (TypeScript)

The taker runs in the browser via the `crates/wasm` package.

### Build

Build with [`wasm-pack`](https://rustwasm.github.io/wasm-pack/). Pick the target
that matches your toolchain — `web` for native ES modules, `bundler` for
webpack/vite/rollup, `nodejs` for Node:

```sh
wasm-pack build crates/wasm --target web --out-dir pkg
```

This emits `crates/wasm/pkg/` containing `stroemnet_wasm.js`, the TypeScript
definitions `stroemnet_wasm.d.ts`, and `stroemnet_wasm_bg.wasm`.

### Minimal example

```ts
import init, {
  StroemGateway,
  getDefaultConfig,
  type CheckedQuote,
  type SwapStatusUpdate,
} from "./pkg/stroemnet_wasm.js";

await init(); // load the wasm module

// Start from the default config and tweak it.
const config = getDefaultConfig();
config.bootstrapPeers = ["wss://a-known-node.example/"];
// config.handler.spreadPercent = 0.5;  // optional tuning

const gateway = new StroemGateway(config);

// Subscribe before connecting — quotes/status arrive asynchronously.
gateway.onPeerCount((count: number) => console.log("peers:", count));
gateway.onQuote((quote: CheckedQuote) => {
  console.log("quote out:", quote.amount_out, "sig valid:", quote.signature_valid);
  // To accept: build a CommitmentV1 from the quote and call
  // await gateway.submitCommitment(commitment, secret);
});
gateway.onSwapStatus((update: SwapStatusUpdate) => {
  console.log("swap", update.swap_id, "stage:", update.stage);
});

await gateway.connect();

// Request a quote. origin/destination are ChannelId bytes:
//   0 = kaspa-tn10, 1 = ethereum-sepolia, 2 = igra-galleon
const swapId = crypto.getRandomValues(new Uint8Array(32));
await gateway.requestQuote(swapId, 1, 0, "0.05"); // 0.05 ETH (Sepolia) → KAS
```
