# Plan: USB NCM transmit path → ICMP echo reply

Goal: make the USB NCM NIC reply to ICMP echo requests at IP `10.10.10.83`.

The receive side is already wired: `nic.rs::poll_bulk_in` reads NTB headers
but doesn't yet walk the NDP. Outbound is entirely missing — there is no
bulk-OUT endpoint, no NTB builder, and no IP/ARP/ICMP code.

QEMU does not emulate a USB NCM device, so end-to-end behavior must be
verified against real hardware. Strategy: every milestone ends with a
**concrete, observable behavior on the host machine** so we never go more
than one milestone before catching a hardware-side regression.

## Conventions used throughout

- Our MAC: whatever `iMacAddress` returns from the device (already fetched
  in `run`). For tests we'll capture it once and hard-code in checks.
- Our IPv4: `10.10.10.83/24`.
- Manual host setup (Linux peer connected to the dongle):
  ```
  sudo ip addr add 10.10.10.1/24 dev <ifname>
  sudo ip link set <ifname> up
  sudo tcpdump -i <ifname> -nn -e
  ```
  `<ifname>` is whatever name the kernel assigns to the NCM dongle.

## Source layout

All new modules sit flat under `src/` (matching the existing convention
on this branch — `usb.rs`, `xhci.rs`, `nic.rs`, …). No subdirectory
module trees.

Files added across the milestones:

| File | Introduced in | Contents |
|---|---|---|
| `src/ncm.rs` | M1 | NTB16 build / parse (NCM-layer framing) |
| `src/eth.rs` | M1 | `EthernetAddr`, `EthernetType`, `EthernetHeader` |
| `src/arp.rs` | M1 | `ArpPacket` |
| `src/ip.rs`  | M1 (small) → M3 (extended) | `IpV4Addr` in M1; `IpV4Packet`, `IpV4Protocol` added in M3 |
| `src/checksum.rs` | M3 | `InternetChecksum` + `InternetChecksumGenerator` |
| `src/icmp.rs` | M3 | `IcmpPacket`, `IcmpType` |

Header structs are `#[repr(packed)]` and implement `Sliceable` (the
trait already in `src/slice.rs`, used by `UsbDeviceDescriptor`).
Headers nest: `IpV4Packet` embeds `EthernetHeader`, `IcmpPacket`
embeds `IpV4Packet`. No trait gymnastics, no `Vec`/`Box` in headers.

Naming is taken from `main`'s `os/src/net/` (`EthernetAddr`,
`EthernetHeader`, `ArpPacket`, `IpV4Packet`, `IcmpPacket`, etc.) so
that field names and struct shapes match the wider project, but the
file/module *organization* is flat to fit this branch.

## Refactor done up-front (minimal)

Only one structural change before M1 starts:

- **Extract the inline NTH parsing in `poll_bulk_in` into a pure
  `ncm::parse_nth16(&[u8])`** as part of creating `src/ncm.rs` in M1.
  No behavior change; one extracted fn so it can be unit-tested without
  hardware.

Nothing else is refactored preemptively. New types/files are added as
each milestone needs them, each landing with its tests.

## Milestone 1 — Send a gratuitous ARP

End state: at startup, wasabi sends one self-announcement ARP onto the
wire, and `tcpdump` on the host shows it.

Why this end state: it exercises the *entire* TX path (xHCI bulk-OUT +
NCM framing + Ethernet construction) with the smallest possible payload
that a real NIC will actually transmit. Anything less (raw bytes, empty
NTB) risks the device stalling the endpoint and giving misleading
results.

### Code changes

1. **`src/xhci.rs`**: add `EndpointContext::new_bulk_out_endpoint`
   (mirror of `new_bulk_in_endpoint`, EP type = `BulkOut`) and
   `NormalTrb::new_out` (same as `new_in` minus the
   `CTRL_BIT_DATA_DIR_IN` bit).
2. **`src/usb.rs`**: extend `configure_endpoint` so
   `is_bulk_endpoint() && !is_dir_in()` calls the new ctx ctor instead of
   returning `Err("Unsupported ep type / dir")`.
3. **`src/nic.rs`**: locate the bulk-OUT endpoint descriptor (interface
   1 alt 1, bulk, OUT), pass it into `configure_endpoint`, hold its
   ring alongside the bulk-in ring.
4. **`src/eth.rs`** (new): `EthernetAddr`, `EthernetType` (`ip_v4()`,
   `arp()`), `EthernetHeader` — all `#[repr(packed)]` + `Sliceable`.
5. **`src/ip.rs`** (new, small in M1): `IpV4Addr` newtype around
   `[u8; 4]` with `new`, `bytes`, `Display`/`Debug`. Will grow in M3.
6. **`src/arp.rs`** (new): `ArpPacket` (`#[repr(packed)]` + `Sliceable`,
   embeds `EthernetHeader`). Constructors: `request(src_eth, src_ip,
   dst_ip)` and `gratuitous(eth, ip)`.
7. **`src/ncm.rs`** (new): `parse_nth16(&[u8])` (extracted from
   `poll_bulk_in`'s inline parsing) and `build_ntb16(datagram: &[u8],
   seq: u16) -> Vec<u8>`. Pure functions.
8. **`src/nic.rs`**: `send_datagram(&[u8])` helper that calls
   `ncm::build_ntb16`, pushes a `NormalTrb::new_out` onto the bulk-out
   ring, and awaits the transfer-completion event.
9. **`src/nic.rs`**: at end of `run`, build a gratuitous ARP for
   `10.10.10.83` and call `send_datagram`.

### Unit tests (run in QEMU, no USB needed)

In `#[cfg(test)] mod tests` at the bottom of `src/ncm.rs`:

- `parse_nth16_known_bytes` — round-trip a hand-built NTH16, verify
  fields. Anchors the existing inline parser semantics under a name.
- `build_ntb16_layout` — feed a 42-byte ARP frame, assert exact byte
  layout: NTH16 `"NCMH"` at 0, header length `0x000C` at 4, sequence at
  6, block length at 8, NDP index at 10; NDP16 `"NCM0"` at NDP offset;
  datagram pointer table ending in `(0,0)`. Golden bytes from
  CDC NCM 1.1 §3.2.
- `build_ntb16_minimum_padding` — result honors the conservative 4-byte
  alignment that NCM 1.1 always permits.

In `#[cfg(test)] mod tests` at the bottom of `src/eth.rs`:

- `ethernet_header_layout` — `size_of::<EthernetHeader>() == 14`,
  default zero, broadcast helper produces all-`0xFF`.

In `#[cfg(test)] mod tests` at the bottom of `src/arp.rs`:

- `arp_packet_size` — `size_of::<ArpPacket>() == 42`.
- `arp_request_bytes` — `ArpPacket::request(...)` produces known-good
  bytes (golden bytes hand-derived).
- `arp_gratuitous_bytes` — gratuitous ARP for our MAC + 10.10.10.83
  matches expected byte sequence.

### Manual hardware test

1. Boot wasabi on the target with the NCM dongle plugged in.
2. On the host: `tcpdump -i <ifname> -nn -e arp` running.
3. Expected within ~1s of boot: one line of the form
   `<our-mac> > ff:ff:ff:ff:ff:ff, ARP, Request who-has 10.10.10.83
   tell 10.10.10.83`.
4. Bonus check: `arp -an | grep 10.10.10.83` on the host should now
   show our MAC.

If step 3 fails, suspected order:
1. xHCI Transfer Event reports an error → ring/EP wiring (M1.3).
2. Transfer Event OK but tcpdump silent → NTB layout wrong (M1.4).
3. Transfer Event OK and tcpdump shows malformed bytes → ARP frame
   construction wrong (M1.6).

## Milestone 2 — Walk inbound NDP + answer ARP

End state: the host can resolve `10.10.10.83` to our MAC via ARP.

Why this end state: the host needs ARP resolution before any IP traffic
will reach us. This milestone unlocks the receive-side walker we'll
build on in M3, and verifies that incoming frames are decoded byte-for-byte
correctly — easier to debug now (small frames) than alongside ICMP.

### Code changes

1. **`src/ncm.rs`**: `iter_ntb16_datagrams<'a>(ntb: &'a [u8]) ->
   impl Iterator<Item = &'a [u8]>` — walks the NDP16, yields each
   Ethernet frame. Pure function.
2. **`src/arp.rs`**: add `ArpPacket::is_request_for(IpV4Addr) -> bool`
   and `reply_to(&self, our_eth: EthernetAddr) -> ArpPacket` (op 1→2,
   swap sender/target, fill our MAC into the new sender_mac).
3. **`src/nic.rs::poll_bulk_in`**: replace the current `hexdump` of the
   raw NTB with: parse NTH → walk datagrams → for each frame, dispatch
   on EtherType. For EtherType `0x0806` ARP request to our IP, build a
   reply via `ArpPacket::reply_to` and call `send_datagram`.

### Unit tests

In `src/ncm.rs`:
- `iter_ntb16_datagrams_single` — golden NTB containing one frame; assert
  the iterator yields exactly that frame's bytes.
- `iter_ntb16_datagrams_multi` — two-frame NTB; both yielded in order.

In `src/arp.rs`:
- `arp_is_request_for_match_and_mismatch` — request with matching IP
  returns true, mismatch returns false.
- `arp_reply_to_swaps_and_op` — reply has op `2`, swapped MAC/IP fields,
  and our MAC in `sender_mac` (golden input/output bytes).

### Manual hardware test

1. Host: `arp -d 10.10.10.83 2>/dev/null; arping -c 1 10.10.10.83`
   (or `ip neigh flush dev <ifname> && ping -c 1 10.10.10.83` — the
   ping itself will fail at this milestone, that's expected).
2. Expected: `arping` reports a reply, OR `arp -an` shows our MAC for
   `10.10.10.83`.

If the ARP exchange doesn't complete:
1. tcpdump shows the request but no reply → inbound walker (M2.1) or
   ARP responder (M2.3).
2. tcpdump shows our reply but with malformed bytes → reply builder
   (M2.2).

## Milestone 3 — ICMP echo reply

End state: `ping -c 5 10.10.10.83` from the host gets 5/5 replies.

### Code changes

1. **`src/checksum.rs`** (new): `InternetChecksum` newtype +
   `InternetChecksumGenerator` (chunk-feed API). Lifted in shape from
   `main`'s `os/src/net/checksum.rs`, including its RFC 1071 unit
   tests.
2. **`src/ip.rs`** (extended): add `IpV4Protocol` (`icmp() = 1`) and
   `IpV4Packet` (`#[repr(packed)]` + `Sliceable`, embeds
   `EthernetHeader`). Methods: `src`, `dst`, `protocol`,
   `clear_checksum`, `set_checksum`.
3. **`src/icmp.rs`** (new): `IcmpType` (`request() = 8`, `reply() = 0`),
   `IcmpPacket` (`#[repr(packed)]` + `Sliceable`, embeds `IpV4Packet`).
   Helper:
   - `IcmpPacket::echo_reply_from_request(req: &IcmpPacket, our_eth:
     EthernetAddr, our_ip: IpV4Addr) -> IcmpPacket` — swaps MAC and
     IP, sets type 0, recomputes IP and ICMP checksums via
     `InternetChecksumGenerator`.
4. **`src/nic.rs::poll_bulk_in`** dispatch: add EtherType `0x0800`
   IPv4 + ICMP type 8 to our IP → call `echo_reply_from_request` and
   `send_datagram` the result.

For ICMP we just recompute the checksum over the ICMP payload rather
than doing incremental adjustment. The ICMP payload echoes the request
data, so we need to feed it through the generator anyway — incremental
adjust is a micro-optimization not worth the complexity here.

### Unit tests

In `src/checksum.rs` (lifted from main):
- `internet_checksum` — the worked examples from RFC 1071, including
  empty input → `0xffff`, single-feed and split-feed equivalence.

In `src/ip.rs`:
- `ipv4_packet_size` — header-only size (excluding embedded
  `EthernetHeader`) is 20 bytes.
- `ipv4_header_checksum_known` — set up a packet with known fields,
  recompute checksum, compare against expected (use the RFC 1071
  example bytes).

In `src/icmp.rs`:
- `icmp_packet_size` — additional bytes beyond `IpV4Packet` is 8.
- `icmp_echo_reply_swaps_and_recomputes` — golden request frame in,
  golden reply frame out (manually computed including checksums).

### Manual hardware test

1. Host: `ping -c 5 10.10.10.83`.
2. Expected: 5 packets transmitted, 5 received, 0% loss. RTT will be
   high (many ms) because we're not optimizing the polling loop yet —
   that's fine.
3. `tcpdump -i <ifname> -nn icmp` should show the request/reply pairs
   with matching `id`/`seq`.

If replies don't come through:
1. tcpdump shows requests reaching the host but no replies → check
   that our dispatch matches `dst_ip == 10.10.10.83 && icmp_type == 8`.
2. Replies on the wire but `ping` shows "wrong data!" or checksum
   errors → checksum fix (M3.1).

## Test plan summary

| Layer | Mechanism | When |
|---|---|---|
| Pure byte layout (NTB build/parse, ARP, IP, ICMP) | `#[test_case]` in QEMU | Before each milestone's manual test |
| xHCI bulk-OUT + EP wiring | Manual: tcpdump observes traffic | M1 |
| Inbound NDP walker + ARP | Manual: `arping`/`arp -an` | M2 |
| End-to-end ICMP | Manual: `ping` | M3 |

Run unit tests with `cargo test` (uses the existing
`custom_test_frameworks` runner that boots QEMU and exits via
`exit_qemu`). They never touch a USB device.

## Out of scope (intentional)

- IP fragmentation, multi-datagram NTBs, IP options, TCP/UDP, DHCP,
  multicast, NCM `SET_NTB_*` configuration calls (we use the device's
  defaults from `GET_NTB_PARAMETERS`).
- Driver shutdown on disconnect (already a TODO from the prior session).
- ARP cache (we don't *initiate* IP traffic in this plan, so we never
  need to resolve anything ourselves).
