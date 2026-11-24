# ultar-bus – integration notes

Area: **system core (area 3)**, target folder `system/kern/ultar-bus`. Plan §3.0, §3.0b (L3), §3.6.
Replaces state files in `/run` + polling (old `ultar-notify` log file and the 400 ms loop in `ultar-shell/src/notify.rs`).

## What exists

| Part | File(s) |
|---|---|
| Wire format (preamble, u32-LE frames, `Envelope`, `Kind`) | `src/wire.rs` |
| Errors (`BusError`, wire-level `ErrorCode`) | `src/error.rs` |
| Names and patterns (`devices.*`, `*`) | `src/topic.rs` |
| Identities + `PeerIdentity` trait and 3 implementations | `src/identity.rs` |
| `Policy` / `Rule` (default deny) + TOML-subset reader | `src/policy.rs`, `src/policy_text.rs` |
| Typed layer `Message` / `Topic` / `Call`, `Text` (i18n key + args) | `src/message.rs` |
| First contracts | `src/messages/{devices,notify,settings,power,shell}.rs` |
| Launcher contract (shell, Store: `launcher.list/start/install/uninstall` …) | `src/messages/launcher.rs` – documented in [ultar-launcher.md](ultar-launcher.md) |
| Broker | `src/broker/{mod,conn,route,state}.rs` |
| Client | `src/client/{mod,link,subscription,service}.rs` |
| Daemon `ultar-busd` + ar/en/de message table | `src/bin/ultar-busd.rs`, `src/msg.rs` |

Dependencies: `serde` (derive), `postcard` (`alloc` only; pulls `cobs` → `thiserror`, all pure Rust). Dev only: `toml`.
No tokio, no libc, no `unsafe` (`#![forbid(unsafe_code)]`). Static build verified:
`RUSTFLAGS="-C target-feature=+crt-static" cargo build --release` → static-pie `ultar-busd`, 1.8 MB unstripped.

## Public API (summary)

```rust
// Broker
Broker::builder(policy).config(BrokerConfig{..}).listen(path, Arc<dyn PeerIdentity>)
      .listen_with_mode(path, auth, 0o600).start() -> Result<Broker, BusError>
broker.stats() -> BrokerStats   broker.set_policy(Policy)   broker.paths()   broker.shutdown() / Drop

// Client (Clone; one connection, one reader thread)
Client::connect(path, ClientOptions::named("devd")) -> Result<Client, BusError>
client.call(&settings::Get{..}, timeout) -> Result<GetReply, BusError>            // typed, versioned
client.request::<T, R>(service, method, &T, timeout) -> Result<R, BusError>       // untyped (version 0)
client.publish(&PowerState{..})            // confirmed; retained if T::RETAINED
client.publish_unconfirmed(&msg)           // fire and forget
client.publish_raw(topic, version, retain, bytes, confirm)
client.subscribe::<PowerState>() -> TypedSubscription<T>   // recv / recv_timeout / try_recv / into_raw
client.subscribe_pattern("devices.*") -> Subscription      // yields RawEvent{topic,version,retained,payload}
client.register("settings") -> ServiceEndpoint             // recv / recv_timeout / try_recv / run(handler)
IncomingRequest: method() caller() version() payload() decode::<C>() decode_raw::<T>()
                 reply(&R) reply_raw(&T) fail(ErrorCode) respond::<C>(|call| -> Result<C::Reply, ErrorCode>)
client.identity() is_connected() wait_connected(timeout) close()

// Contracts
trait Message { const VERSION: u16 }
trait Topic: Message { const TOPIC: &str; const RETAINED: bool = false }
trait Call: Message { const SERVICE: &str; const METHOD: &str; type Reply: Message }
```

### How a service uses it (example: notification service)

```rust
let bus = Client::connect(socket, ClientOptions::named("notifyd"))?;
let inbox = bus.register(notify::Post::SERVICE)?;
inbox.run(|request| {
    let _ = request.respond::<notify::Post>(|post| {
        let id = store(&post);
        let _ = bus.publish(&notify::Posted { id, timestamp_ms: now(), post });
        Ok(notify::PostReply { id })
    });
});
```
Shell: `let posted = bus.subscribe::<notify::Posted>()?; loop { let n = posted.recv()?; … }` – blocks, no polling.
App: `bus.call(&notify::Post{..}, Duration::from_secs(2))?`.

## Behaviour that other crates can rely on

- **Wire:** `"UBUS" | version u16 LE | 0u16` once per direction, then `u32 LE length | postcard(Envelope)`.
  `Envelope { kind, id, target, method, version, flags, origin, payload }`, kinds
  `Hello, Welcome, Register, Unregister, Request, Response, Event, Subscribe, Unsubscribe, Error`.
  Max frame 1 MiB by default (`BrokerConfig::max_frame`), enforced on both sides. `PROTOCOL_VERSION = 1`.
- **Order:** all routing runs under one lock → every subscriber sees the events of a topic in the same order.
- **Retained:** last value per topic is replayed to a *new subscription only* (flag `RawEvent::retained`), also after a reconnect.
- **Backpressure:** each connection has a bounded queue (`queue_capacity`, default 1024 frames). An overflowing
  subscriber is dropped with `Error(SlowConsumer)` as last frame; the broker and other clients never block.
  A full *service* queue is not dropped – the caller gets `ServiceBusy`. Client side: full local buffer
  (`ClientOptions::buffer`, default 256) → next `recv` returns `BusError::Lagged(n)` once, then continues.
- **Requests:** broker re-numbers forwarded requests and only accepts the answer from the connection that owns
  the service; `origin` = authenticated caller identity (`IncomingRequest::caller()`).
  Typed failures: `PermissionDenied, UnknownService, UnknownMethod, ServiceBusy, ServiceGone, NoReply,
  VersionMismatch, BadPayload, Service(u32)`; local: `Timeout, NotConnected, Closed`.
- **Permissions:** default deny. Permission is checked before existence (denied callers learn nothing).
  A subscription pattern must be fully covered by a `subscribe` rule. Call rules are written `service.method`.
- **Reconnect:** exponential back-off (50 ms … 2 s), services and subscriptions are re-announced with their old ids;
  a re-announcement the broker refuses ends that subscription (`Remote(PermissionDenied)`, then `Closed`) / endpoint.
  Version mismatch or failed authentication stop the client for good.
- **Idle:** blocking I/O only, no timers. Measured in `tests/idle.rs`: 0 context switches in 1.5 s over all bus threads.
- **Threads:** broker = 1 acceptor per socket + 2 per connection (128/256 KiB stacks); client = 1 reader.

## Policy file (TOML)

```toml
[[rule]]
identity  = "system.settingsd"        # exact, "app.*" or "*"
register  = ["settings"]
publish   = ["settings.changed"]
subscribe = ["devices.*"]
call      = ["notify.post"]           # service.method, "service.*", "*"
```
`Policy::from_toml_str` / `Policy::load` use a built-in reader for exactly this subset (no `toml` runtime dependency).
`Policy`/`Rule` also derive serde, so `toml::from_str::<Policy>` works too; a test proves both agree.

## Authentication

`UnixStream::peer_cred` is **unstable** on rustc 1.97 and the crate forbids `unsafe`, so peer credentials are behind traits:
- `FixedIdentity` – one identity per listening socket; the kernel enforces access through the socket file's
  owner/mode or by mounting only that socket into an app cell. Usable today, used by `ultar-busd --listen PATH=IDENTITY`.
- `TokenIdentity` – secret in the handshake (token file readable only by the program's user).
- `CredentialIdentity<S: CredentialSource>` – uid → identity. **Open:** the HAL must supply the real
  `CredentialSource` (`SO_PEERCRED`); only `StaticCredentials` (test/dev) is included.

## Decisions I took (please confirm)

1. `shell.mode` is **retained** as well (late-starting apps need it); the task only named capabilities and power.
2. Notification body is `Body::{Empty, Localized(Text), Verbatim(String)}` – `Verbatim` only for content written
   by people (SMS/chat text). Titles and all interface text are `Text { key, args }`.
3. Contract struct names: `notify::{Post, PostReply, Posted, Urgency, Category}`, `settings::{Get, GetReply, Set,
   SetReply, Changed, SettingValue}`, `devices::{Capabilities, Capability, Availability, DriverState, DriverPhase}`,
   `power::{PowerState, PowerSource, PowerProfile, ThermalLevel}`, `shell::{ShellMode, Mode}`. All `VERSION = 1`.
4. `ultar-busd` needs `--policy FILE` or explicit `--allow-all`; without either it refuses to start.

## Test results (rustc 1.97.0, x86_64 Linux, 2 cores)

`cargo fmt --check` clean · `cargo clippy --all-targets -- -D warnings` clean · `cargo doc` with `-D warnings` clean.
`cargo test -j1` run **5 times in a row: 139 passed, 0 failed, 1 ignored each time.**

| Binary | Tests |
|---|---|
| lib unit tests | 73 |
| `ultar-busd` unit tests (argument parsing) | 5 |
| `tests/request.rs` | 12 |
| `tests/pubsub.rs` | 11 |
| `tests/permissions.rs` | 10 |
| `tests/robustness.rs` | 15 |
| `tests/reconnect.rs` | 9 |
| `tests/load.rs` (50 clients, fuzz loop with fixed xorshift seed) | 2 (+1 ignored benchmark) |
| `tests/idle.rs` | 1 |
| doc test (compile only, `no_run`) | 1 |

Benchmark (`cargo test --release --test load -- --ignored --nocapture`, two runs, shared 2-core machine):
request/response mean 36–53 µs, p50 33–40 µs, p99 69–146 µs (19–28 k calls/s);
publish/subscribe 165–180 k messages/s (64-byte payload, one subscriber).

## Not done / not tested – honest list

- **Ultar kernel not tested.** The implementation relies on Linux semantics that the kernel must provide for
  `AF_UNIX` stream sockets: `shutdown(SHUT_RD)` wakes a thread blocked in `read`, `SO_RCVTIMEO`/`SO_SNDTIMEO`
  (std `set_read_timeout`/`set_write_timeout`), `connect` to a stale socket file fails. No aarch64 build was tried.
- Real `SO_PEERCRED` source missing (see above).
- Eviction of a real `Client` by a real broker (→ `Lagged` + reconnect) is only tested against a scripted fake broker;
  the broker side of eviction is tested with raw sockets.
- `ServiceBusy`, subscription/retained limits and lazy purge of unanswered requests (`pending_ttl`) are unit-tested
  on the router only, not over sockets. `Lang::from_env` and `ultar-busd`'s `main` are only smoke-tested by hand.
- A burst larger than `queue_capacity` drops even a healthy subscriber that is momentarily not scheduled
  (this is the specified bounded-queue behaviour; high-rate publishers must pace themselves or the capacity be raised).
  There is no per-subscriber flow control.
- `Broker::set_policy` affects new operations only; existing subscriptions/registrations are not revoked.
- The TOML subset reader is slightly more lenient than TOML in one spot (`[ [rule] ]` with spaces between brackets).
- Not part of this crate yet: shared-memory hand-over for large buffers (§3.6), bus/socket activation of services,
  signal handling in `ultar-busd` (std has none; on SIGTERM the socket file stays and is replaced as stale on next start),
  clearing a retained topic, a `[workspace]` table still in `Cargo.toml` (coordinator removes it).
