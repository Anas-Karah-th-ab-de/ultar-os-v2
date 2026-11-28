# Network isolation of cells (stage P0 of `docs/FREMD-APPS.md`)

KM Code Craft · Ultar OS v2 · 2026-11-28 · kernel `kernel/ultar-kern/src/isolation/net.rs`, `socket.rs`,
`isolation/calls.rs`, `syscall.rs` (`sys_fork`, Unix names), `proc.rs` (`PR_SET_PDEATHSIG`) · HAL
`system/hal/ultar-sys` · launcher `system/core/ultar-launcher` · test `kernel/ultar-kern/user/zellnetz.c`,
`tools/kern-test/zell-netz-e2e.sh` (verdict `ZELL-NETZ: ALLES OK`, part of `tools/kern-test/ci.sh`).

Before an app from another system may run, a cell must not reach anything of the device through the network:
not the services on the host's loopback (local model server, web console, the bus is a Unix socket and stays
outside the cell's view anyway), not the device's own address, not the local network.

## 1. What the kernel offers

| Piece | Behaviour |
|---|---|
| **Network namespace** – `unshare(CLONE_NEWNET)`, `clone(CLONE_NEWNET)` | needs the `MOUNT` right (like the other namespaces); a group with more than one thread gets `EINVAL`. The namespace has only **its own loopback**: its `127.0.0.0/8` ports are its own (a cell may listen on the same port as a host service), abstract Unix names are its own, nothing reaches the host's loopback (`ECONNREFUSED`, there is no listener in the namespace) and nothing leaves (`ENETUNREACH`, also DNS). A listening socket in a namespace is never reachable from the host or the network. |
| **"Internet only" barrier** – `prctl(PR_ULTAR_NET_GUARD = 0x554c5202)` | a per-process mark like `no_new_privs`: inherited by every child, kept across `execve`, never cleared (there is no option to clear it). Read it with `PR_ULTAR_NET_GUARD_GET (0x554c5203)` → 1/0. A marked process may send to the internet only: `127.0.0.0/8` of the host, the device's own address, the own network's broadcast, `0/8`, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `100.64/10` and `224/4`–`255/8` are refused with `EACCES`. **DNS:** the DNS servers of the kernel's network configuration (DHCP lease or static, `/sys/class/net/<if>/ultar_ipv4`) are reachable on port 53 (UDP and TCP), also when they are on the LAN (a home router) – nothing else on those addresses. |
| barrier **in a namespace of its own** | the namespace's own loopback, plus the way out under the rules above; answers from the network (TCP of its own connections, UDP from peers it sent to) come back in; no new connection from outside. This is what the launcher gives a network app outside the system profile. |
| barrier **in the host namespace** | additionally: no `listen` and no `bind` other than `0.0.0.0:0` (an outgoing socket) – the process can never offer a service on the device's addresses. |
| **Sockets keep both** | a socket belongs to the namespace of its creator and remembers its creator's barrier: passing it to another process (`SCM_RIGHTS`) does not lift the rules. |
| `PR_ULTAR_NET_NS_GET (0x554c5204)` | the caller's namespace number (0 = host), for tests and diagnostics. |
| **`PR_SET_PDEATHSIG` / `PR_GET_PDEATHSIG`** | now effective: when a thread group ends, every child thread that asked for a signal gets it (a child starts without one). Linux sends it when the parent *thread* ends; the Ultar kernel when the parent *process* ends – the launcher's stage 1 is the child of a single-threaded launcher process, so both are the same there. |
| **set-user-id** | the Ultar kernel never honours set-user/group-id bits at `execve`: `nosuid` is always in effect, nothing to add. |

How it works inside: every socket carries its namespace; loopback frames carry the namespace they were sent
in and reach only sockets of that namespace; frames from the network card are the host's and reach a socket
of another namespace only as the answer to what that socket sent out (its 4-tuple, or a UDP peer it wrote
to). ARP, the kernel's DHCP client and the mesh listener see only the host's frames.

## 2. Launcher

* Probe (`ultar-launcher --probe`): bit `PROBE_NET` (2 → `net`) as before; new bit `PROBE_NET_GUARD` (16)
  when the probe process could set the barrier inside its network namespace → feature `net-guard`
  (`launcher.platform` record `net_guard`).
* A start (`plan.rs::network`):

  | App | Network |
  |---|---|
  | without the `network` permission | own namespace, nothing out (needs `net`, as before) |
  | with it, profile `app.system` (signed by KM Code Craft, e.g. a browser that must open a router's page) | host network, as before |
  | with it, any other profile (`app.standard`, later `app.foreign`) | own namespace **with the barrier** (needs `net` and `net-guard`); stage 1 sets it after `unshare` and reads it back, else the start fails with the namespace error |

  A kernel without the barrier stops such a start (`ERROR_PLATFORM`, feature `net-guard`) unless the operator
  lists `net-guard` in `accept_missing` (shipped: empty). Linux has no barrier: there the launcher tests accept
  it explicitly (`tests/cell.rs`), and `a_granted_store_app_needs_the_barrier` checks the refusal.
* `CellPlan` has `net_guard` (`--net-guard internet` on the helper's command line).

## 3. Tests

`zellnetz` on the Ultar kernel in QEMU with a public test range (RFC 5737, `net=198.51.100.0/24`), so "the
internet" is a `guestfwd` service inside the test machine (`198.51.100.100:8080` answers `INTERNET-OK`):
namespace without the barrier (host loopback service and abstract name refused, no internet, no DNS, own
loopback carries data on the host service's port, a datagram to the host's UDP port stays inside); namespace
with the barrier (internet answers, `127.0.0.1` is its own, own address / five LAN ranges / broadcast,
multicast and subnet broadcast refused, DNS server on 53 allowed and its answer comes in); a cell's listener on
`0.0.0.0` reachable neither on the host's loopback nor on the device's address; barrier in the host namespace
(host loopback, own address, fixed port, listen refused; `0.0.0.0:0` and the internet allowed); the barrier
survives fork + `execve`; the host never got it; `clone(CLONE_NEWNET)`; `CLONE_NEWNET` without `MOUNT` →
`EPERM`; `PR_SET_PDEATHSIG` ends an orphan with its parent, without it the orphan lives on.

Found along the way (a kernel bug older than this stage): a blocking `connect` could wait for good when the
answer was handled inside the call – a local RST (closed port on a loopback) or SYN-ACK came back within the
call's own `poll()`, and the wait then started from the generation after it. The wait now starts from the
generation before the attempt, and a refusal seen inside the call is returned at once (`ECONNREFUSED`).

## 4. Not done / open

* No IPv6 at all in the kernel (`AF_INET6` → `EAFNOSUPPORT`); the barrier rules for IPv6 come with IPv6.
* `/proc/net` (the socket list) is not filtered per namespace yet: a cell can read the host's socket table
  (addresses and ports, no data).
* The host answers a connection to its own non-loopback address on a closed port silently (no reset): a cell's
  listener is not reached, the connector waits for its timeout (old behaviour, unchanged).
* A namespace cannot be entered by another process (`setns` knows only mount and pid namespaces).
* LAN access for an app that needs it (printer, NAS) is a later, explicit per-app switch (`docs/FREMD-APPS.md`
  §5); system apps keep the host network until then.
