# Plan: Simple TCP → remote terminal access

Goal: open a TCP listener on `10.10.10.83:23`, accept one client at a
time, and pipe wasabi's terminal (the same `Console` that handles
keyboard input + `print!`/`println!` output) over the connection.

What we already have, and how it shapes the plan:

- IPv4 + ICMP framing already exists (`src/ip.rs`, `src/icmp.rs`,
  `src/checksum.rs`). TCP can reuse `IpV4Packet` and
  `InternetChecksumGenerator`.
- `nic.rs::poll_bulk` already dispatches per-frame on EtherType /
  IPv4 protocol. Adding TCP is a third arm next to ARP and ICMP.
- The transmit path runs on `bulk_out_ring`, owned by `poll_bulk`.
  Reactive responses (ARP reply, ICMP reply) are pushed into a
  `replies` Vec that's flushed at the bottom of the rx loop. TCP rx
  responses fit the same shape; *unsolicited* TCP transmits (terminal
  output piling up while no segments are arriving) need a new outbound
  queue we'll drain on the same loop iteration.
- main's `os/src/net/tcp.rs` is the structural reference. We mirror
  the field/method names but skip TIME_WAIT, retransmission, RTT
  estimation, multi-socket tables, congestion control, and the
  Network manager. One global TcpSocket on a fixed port is enough for
  one terminal session at a time.

QEMU still doesn't emulate a USB-NCM NIC, so TCP behavior is verified
against real hardware. Each milestone ends with a concrete observable
on the host side.

## Conventions

- Listener address: `10.10.10.83:23`. Port 23 is the standard telnet
  port; we don't implement the telnet *protocol* (no IAC negotiation),
  just raw bytes. `nc 10.10.10.83 23` is the test client. `telnet`
  itself works too if started with `telnet -E` (no escape) — the
  initial IAC bytes the server doesn't understand are echoed and
  ignored.
- One connection at a time. After a clean close (FIN / FIN+ACK), the
  socket returns to LISTEN.

## Source layout (additions)

Flat, like the rest of the network code:

| File | Introduced in | Contents |
|---|---|---|
| `src/tcp.rs` | M1 (header), M2 (socket) | `TcpPacket`, `TcpSocketState`, `TcpSocket` |

No other new files. `nic.rs` gains a TX queue + dispatch arm + a poll
task; `print.rs` gains an optional TCP sink; `cui.rs` exposes a Console
constructor that takes already-parsed bytes (one-byte-at-a-time).

## Out of scope

- Active (client) open. Server-only — the `SynSent` and `Closing`
  branches of main's state machine aren't implemented.
- Retransmission, RTO, RTT estimation, fast retransmit, SACK,
  duplicate-ACK handling. We rely on the absence of packet loss on a
  direct USB-Ethernet link.
- Multiple simultaneous connections. One global `TcpSocket`.
- Window management. We always advertise `0xFFFF`. The remote sender
  has to be reasonable about not overflowing us — for the keystroke /
  command-output traffic this terminal sees, that's automatic.
- TIME_WAIT. After the four-way close completes we go straight to
  LISTEN.
- IP fragmentation. TCP segments are built to fit in a single NCM
  frame (the NIC's MTU is well above what a terminal session needs).
- TCP options other than what's already in the SYN we receive (we
  reflect MSS if present in the SYN; otherwise default).

## Milestone 1 — TCP packet header type

End state: `src/tcp.rs` exists with `TcpPacket` (`#[repr(packed)]`,
`Sliceable`, embeds `IpV4Packet`), flag accessors, header-length
helpers, and a TCP checksum helper that knows about the IPv4
pseudo-header. No wiring yet; pure module with unit tests.

### Code

1. `src/tcp.rs`:
   - `TcpPacket { ip: IpV4Packet, src_port, dst_port, seq_num, ack_num,
     flags: [u8; 2], window, csum: InternetChecksum, urgent_ptr }` —
     20-byte TCP header on top of the 14+20 byte Ethernet+IPv4 prefix.
   - getters/setters: `src_port`, `dst_port`, `seq_num`, `ack_num`,
     `window`, `header_len_bytes`, `set_header_len_nibble`.
   - flag helpers: `is_syn`, `is_ack`, `is_fin`, `is_rst`, `set_syn`,
     `set_ack`, `set_fin`, `set_rst`.
   - `tcp_segment_checksum(segment: &[u8], src: IpV4Addr, dst: IpV4Addr)
     -> InternetChecksum` — feeds the segment then the pseudo-header
     (src, dst, [0, protocol=6], length) per RFC 793 §3.1.

### Unit tests

- `tcp_packet_header_size_is_20` — asserts
  `size_of::<TcpPacket>() - size_of::<IpV4Packet>() == 20`.
- `tcp_flags_roundtrip` — set each of SYN/ACK/FIN/RST, read back,
  verify exactly that bit is set.
- `tcp_header_len_nibble_round_trip` — set 5, observe `header_len_bytes
  == 20`.
- `tcp_segment_checksum_self_check` — build a SYN segment with our
  helper, confirm that feeding the segment+pseudo-header back through
  the generator yields `0x0000` (the standard self-check). This
  catches any mistake in our pseudo-header construction.

### Manual test

None — pure module.

## Milestone 2 — TCP echo server

End state: wasabi accepts a TCP connection on `10.10.10.83:23` and
echoes back whatever bytes the client types. After the client sends
FIN, the connection closes cleanly and the listener is ready again.

This milestone gets the wire protocol working in isolation, before
we tangle it with the Console. If a checksum or sequence-number bug
exists, it surfaces here without the terminal noise.

### Code

1. `src/tcp.rs`:
   - `TcpSocketState` enum: `Listen, SynReceived, Established, CloseWait,
     LastAck` (the five states a passive socket actually goes through;
     the rest of main's enum is omitted for now).
   - `TcpSocket { state, peer_ip, peer_port, my_next_seq,
     last_seq_to_ack, rx_data: VecDeque<u8>, tx_data: VecDeque<u8>,
     listen_port: u16 }`. All non-trivial fields under a `Mutex`.
   - `TcpSocket::new_server(port: u16)`.
   - `TcpSocket::handle_rx(&self, segment: &[u8]) -> Option<Vec<u8>>`
     — drives the state machine for one received segment; returns the
     immediate reply bytes (SYN+ACK, ACK, FIN+ACK, etc.) if any. For
     received data, also pushes it to `rx_data` and ACKs.
   - `TcpSocket::poll_tx(&self) -> Option<Vec<u8>>` — if Established
     and `tx_data` non-empty, drain it into a single segment, return
     bytes. Used by the periodic TX task.
   - `TcpSocket::build_segment(&self, syn, fin, ack, data) ->
     Vec<u8>` — internal helper that emits a complete
     Ethernet+IP+TCP+data frame with checksums set. (We don't have a
     peer-MAC table so we reuse the requester's source MAC saved
     in the socket state on SYN.)

2. `src/nic.rs`:
   - `static NET_TX_QUEUE: Mutex<VecDeque<Vec<u8>>>` for unsolicited
     outbound frames.
   - `static TCP_SOCKET: ... = TcpSocket::new_server(23)` (or a `OnceCell`
     pattern matching the codebase's conventions).
   - In `poll_bulk`'s dispatch, add a third arm for IPv4 + protocol=6:
     when `dst_ip == OUR_IP && dst_port == 23`, hand the frame to
     `TCP_SOCKET.handle_rx`, push any returned reply into `replies`.
   - At the bottom of the rx loop, *also* drain `NET_TX_QUEUE` and send
     each frame via `send_datagram`.
   - Spawn a `poll_tcp_tx` task that loops every 20 ms calling
     `TCP_SOCKET.poll_tx()` and pushing any result into `NET_TX_QUEUE`.

3. For this milestone only — make `handle_rx` *also* echo: when in
   Established and the segment carries data, in addition to ACKing it,
   copy the same bytes onto `tx_data` so the next `poll_tx` ships them
   back. We delete this echo line in M3.

### Unit tests

In `src/tcp.rs` `#[cfg(test)] mod tests`:

- `handle_rx_listen_to_syn_received` — feed a crafted SYN segment to
  a fresh server socket; assert the returned bytes are a SYN+ACK with
  `ack_num == client_seq + 1` and the socket transitioned to
  SynReceived.
- `handle_rx_synreceived_to_established` — preset the socket to
  SynReceived; feed the corresponding ACK; assert state becomes
  Established and no reply (or just an ACK with no data).
- `handle_rx_data_acks_and_buffers` — preset Established; feed a
  segment with payload `b"hello"`; assert returned bytes ACK with
  `ack_num == client_seq + 5` and that `rx_data` now contains
  `b"hello"`.
- `handle_rx_fin_to_lastack_then_close` — preset Established; feed
  FIN; assert FIN+ACK reply and state becomes LastAck. Then feed the
  client's final ACK; assert state returns to Listen and peer fields
  are cleared.

### Manual test

```sh
nc 10.10.10.83 23
```

Type some lines. Each line should be echoed back. Press Ctrl-D (or
just terminate `nc` with Ctrl-C) and reconnect to confirm the listener
is usable again.

If something goes wrong:

1. **`nc` hangs immediately on connect** — SYN+ACK isn't getting back
   to the host. Suspect the M1 checksum helper or the way M2 fills the
   peer MAC into the reply Ethernet header. `tcpdump -i <ifname> -nn
   tcp port 23` will tell you whether anything goes out at all.
2. **Connection establishes but typing produces nothing** — the
   periodic `poll_tcp_tx` task isn't running, or `tx_data` isn't being
   populated. Check the wasabi serial console for any TCP-related
   warnings and add a temporary `info!` in the echo-back line.
3. **`nc` reports "Connection reset"** — we ACKed with a wrong
   `seq_num` or `ack_num`. The `handle_rx_data_acks_and_buffers` unit
   test should have caught the most common variants of this; if it
   slipped through, packet-capture and compare seq/ack numbers byte
   by byte against the unit-test golden values.

## Milestone 3 — Hook the terminal up

End state: `nc 10.10.10.83 23` shows the wasabi prompt, accepts
typed commands, and prints output the same as a local keyboard
session would.

### Code

1. `src/print.rs`:
   - Add an optional second sink to `global_print`: a callback (or
     simply a globally-known reference to `TCP_SOCKET`) that, when the
     socket is Established, appends the formatted bytes to its
     `tx_data`. Existing serial+VRAM behavior unchanged.

2. `src/cui.rs`:
   - Expose `Console::feed_byte(&mut self, b: u8)` (or wire the
     existing `handle_key_down` to an ASCII byte) so we can drive the
     console from arbitrary `u8` input rather than only from
     `KeyEvent` values produced by USB/PS2.

3. `src/nic.rs`:
   - Remove the M2 echo line.
   - Spawn a per-connection task (or extend `poll_tcp_tx`) that, while
     the socket is Established:
     - drains `rx_data` byte by byte into a fresh `Console`'s
       `feed_byte`.
     - On state transition Established → CloseWait, drop the Console.

### Unit tests

Nothing pure-testable in this milestone (it's all integration). The
M2 unit tests still hold and continue to exercise the wire layer.

### Manual test

```sh
nc 10.10.10.83 23
```

Expected:
- The connection prints whatever wasabi's startup banner / current
  prompt is.
- Typing `help` (or any command the local console accepts) returns
  the same output as on the local screen.
- Output appears at both the local screen and the remote `nc`
  simultaneously (since `global_print` tees).

If something goes wrong:

1. **Connection establishes but no prompt is printed** — the print-tee
   isn't appending to `tx_data`, or the Console isn't drawing its
   prompt at startup. Check that the new sink is actually called
   (temporary `info!` in `global_print` when the sink fires) and that
   `poll_tcp_tx` is running.
2. **Typed bytes don't make it into the console** — the rx-pump task
   isn't running, or `Console::feed_byte` isn't translating bytes to
   the right `KeyEvent`. Test by feeding a known string through `nc`
   and adding a temporary `info!` showing each byte at the dispatch
   point.
3. **Output is duplicated or garbled** — the print-tee is firing
   recursively (e.g., logging from inside the sink). Make sure the
   sink doesn't itself call `print!`.

## Test plan summary

| Layer | Mechanism | When |
|---|---|---|
| TCP packet layout / flags / checksum | `#[test_case]` in QEMU | M1 |
| Socket state machine | `#[test_case]` driving `handle_rx` with crafted segments | M2 |
| Wire-level interactivity (echo) | Manual: `nc` echo round-trip | M2 |
| End-to-end terminal | Manual: `nc` + run a wasabi command | M3 |

Run unit tests with `cargo test`. They never touch a USB device or
the network.
