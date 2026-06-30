# Plan: consolidate ch4 (NCM driver) by splitting driver-vs-protocol

Status: DRAFT (proposal under review — not yet agreed). Move to plan/active
once the author picks an option below.

## Principle (author's framing)

"NCM-related fixes — code that essentially touches only Ethernet-and-below —
belong in ch4. Even when such a change is currently bundled into one commit
with protocol-level changes, the commit can be split so the link-layer part
becomes ch4."

Working boundary:

- **ch4 / link layer ("Ethernet and below")** — NCM/USB driver MECHANICS:
  bulk-IN/OUT transfers, NTB framing/parsing, buffer sizing, transfer
  timeout & recovery, endpoint config, the xHCI plumbing they use. Files:
  nic.rs, ncm.rs, xhci.rs, eth.rs.
- **ch5 / protocol** — ARP/IP/UDP/ICMP/DHCP/TCP/DNS logic, AND the driver's
  rx **dispatch** to those handlers (e.g. "reply to ARP", "reply to ICMP",
  IP/DHCP config), even when that code lives in nic.rs.

Key subtlety: classification is by the **nature of the change**, not the
file. nic.rs holds both driver mechanics and protocol dispatch.

## Current state (positions in book2 commit order)

- Early ch4 driver block: 88, 90-94 (NCM driver, bulk-OUT, NTB). Clean,
  already contiguous, already before ch5.
- Scattered ch4: 98, 99, 101.
- ch4 hardening block: 117-122 (contiguous), pinned deep in the ch5 region.
- The rest (95-116, 123-136) is ch5 protocol, interleaved.

## Classification of the boundary commits

Pure driver-mechanics (cleanly ch4):
- 99  Walk inbound NTB datagrams (ncm.rs only)
- 117 Increase NCM bulk-IN buffer
- 119 Log bytes_rx/tx in timeout warning
- 120 Wait, don't recycle, on timeout
- 121 Decouple rx/tx so stuck bulk-OUT can't block rx
- 122 Recover stuck bulk-OUT via Stop Endpoint + Set TR Dequeue (nic+xhci)

Mixed — driver mechanics bundled with an unrelated protocol change (SPLIT):
- 118 (ch4) nic.rs = bulk timeout/recovery (ch4) + a small tcp.rs hunk (ch5)
- 123 (ch5) nic+xhci recovery-DCS (ch4) + checksum.rs/tcp.rs (ch5)

Protocol/dispatch that merely *touches* nic.rs (stays ch5, do NOT split):
- 102 ARP reply, 107 ICMP reply, 112 TCP listen, 113-115 TCP, 124 ping,
  126/130/131/132 console/IP/DHCP/routing, 133 DNS. Their nic.rs hunks are
  protocol dispatch/glue, not driver mechanics.

Cross-layer — need an author decision:
- 95  "Add Ethernet header types" (eth.rs) — currently ch5. By the
  "Ethernet and below" rule this is arguably ch4. (Question 1.)
- 98  "Send gratuitous ARP from NCM driver at startup" — currently ch4, but
  it `use crate::arp::ArpPacket` and calls `ArpPacket::gratuitous`, so it
  depends on ch5's arp.rs (97). Driver *startup* action, protocol *payload*.
  (Question 2.)
- 101 "Merge NCM rx and tx into a single poll_bulk loop" — currently ch4.
  The loop is driver mechanics, but it dispatches ARP replies, so it
  references protocol. The hardening block (117-122) modifies this *merged*
  loop, so the hardening is structurally pinned after 101, and 101 is
  pinned after arp.rs. (Question 3 — this is the crux of full contiguity.)

## The coupling problem

The NCM rx loop (`UsbNcmDriver::poll_bulk`) both moves bytes (ch4 mechanics)
and dispatches frames to protocol handlers (ch5). Commit 101 introduces the
merged loop *with* ARP dispatch; the hardening (117-122) then patches that
merged loop. So:

  early driver (88-94) -> 101 (merge + ARP dispatch) -> hardening (117-122)

To put the hardening into a ch4 block *before* ch5, 101 itself must be split
into (a) the merged-loop mechanics (ch4) and (b) the ARP dispatch (ch5).
That split is intricate (both live in one function).

## Two options

### Option 1 — full contiguity (aggressive)
Split 118, 123, AND 101 (loop-mechanics vs ARP-dispatch); re-classify eth.rs
-> ch4 and resolve gratuitous-ARP; then gather every driver-mechanics commit
into one contiguous ch4 block after ch3 and before ch5. Protocol parts +
dispatch move to ch5.
- Pro: ch4 is a single clean run before ch5.
- Con: must untangle poll_bulk dispatch (101); the hardening diffs have to
  be replayed onto a dispatch-free loop -> real conflict-resolution work;
  reorders the historical "hardening came after TCP testing" narrative.

### Option 2 — clean labels, two-phase ch4 (recommended)
Split only the clearly-separable mixed commits so every commit is purely one
layer:
- 118 -> ch4 (nic timeout/recovery) + new ch5 (tcp tweak)
- 123 -> new ch4 (nic/xhci recovery-DCS) + ch5 (checksum/tcp)
Then tidy within dependency limits: keep the early ch4 driver block (88-94),
and keep a *contiguous* late ch4 hardening block (117-122 + the split-out
ch4 parts) where it sits after the protocol it depended on. Optionally move
99 next to the early block.
- Pro: every commit is cleanly one layer; low risk; preserves the natural
  "build driver -> build protocol -> harden driver (bugs found under TCP)"
  story; no poll_bulk untangling.
- Con: ch4 remains in two regions (early + hardening), not a single run.

## Mechanics (same for either option)

- Interactive rebase. To split a commit: mark it `edit`, then
  `git reset HEAD^`, stage the ch4 files and commit (ch4 title), stage the
  ch5 files and commit (ch5 title). Each new commit gets its own Change-Id.
- Reorder by authoring the rebase todo.
- Verify EVERY commit with `scripts/check.sh` (`git rebase --exec`), incl.
  the ping_to_gateway integration test.
- Per split: the two new commits' combined tree == the original commit's
  tree. Final HEAD tree must be byte-identical to pre-work (git diff empty).
- Then `make fix` + `ajimi check --skip-checks order` + push (force).

## Open questions for the author

1. eth.rs (Ethernet header types) — move to ch4, or leave in ch5?
2. "gratuitous ARP" (98) — it sends an ARP at driver startup but uses
   arp.rs. ch4 (driver startup) pinned after arp, or re-label ch5?
3. Pursue full single-block contiguity (Option 1, split poll_bulk/101), or
   clean-labels two-phase (Option 2)?

## Recommendation

Option 2. It realizes the author's intent (every NCM-mechanics change lands
in ch4, protocol bits split out) with low risk and without rewriting the
driver/protocol co-evolution narrative. Revisit Option 1 only if a single
physical ch4 block is required.
