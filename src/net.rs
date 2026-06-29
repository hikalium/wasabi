use crate::arp::ArpPacket;
use crate::executor::sleep;
use crate::hpet::global_timestamp;
use crate::icmp;
use crate::ip::IpV4Addr;
use crate::nic;
use crate::nic::PingPending;
use crate::result::Result;
use crate::slice::Sliceable;
use core::time::Duration;

/// ICMP echo payload length (bytes) used by [`ping_once_result`].
pub const PING_PAYLOAD_LEN: usize = 32;
const PING_REPLY_TIMEOUT: Duration = Duration::from_millis(1000);
const PING_ARP_WAIT: Duration = Duration::from_millis(200);

/// Send one ICMP echo request to `target` and wait for the matching reply.
///
/// Returns `Ok(Some((rtt, src)))` when a reply arrives within the timeout,
/// `Ok(None)` on timeout. An `Err` is only produced for setup failures
/// (NIC not ready yet, or the next hop's MAC could not be resolved). This
/// is the reusable core shared by the `ping` command and the
/// `ping_to_gateway` integration test.
pub async fn ping_once_result(
    target: IpV4Addr,
    seq: u16,
) -> Result<Option<(Duration, IpV4Addr)>> {
    let our_mac = nic::our_mac().ok_or("NCM not ready (no MAC)")?;

    // Route to the destination directly when it is on our subnet,
    // otherwise hand the frame to the DHCP-learned default router. Only
    // the link-layer next hop changes; the ICMP packet still targets
    // `target`. Resolve that next hop's MAC, prodding the network with
    // an ARP request if it is not cached yet.
    let next_hop = nic::next_hop(target);
    let dst_mac = match nic::arp_lookup(next_hop) {
        Some(m) => m,
        None => {
            nic::enqueue_tx_frame(
                ArpPacket::request(our_mac, nic::our_ip(), next_hop)
                    .as_slice()
                    .to_vec(),
            );
            sleep(PING_ARP_WAIT).await;
            nic::arp_lookup(next_hop).ok_or("ARP unresolved")?
        }
    };

    let id: u16 = 0x1d10;
    let payload = [0xa5u8; PING_PAYLOAD_LEN];
    let frame = icmp::echo_request_frame(
        our_mac,
        nic::our_ip(),
        dst_mac,
        target,
        id,
        seq,
        &payload,
    );
    let sent_at = global_timestamp();
    *nic::PING_PENDING.lock() = Some(PingPending {
        id,
        seq,
        sent_at,
        reply_rtt: None,
        reply_src: None,
    });
    nic::enqueue_tx_frame(frame);

    let deadline = sent_at + PING_REPLY_TIMEOUT;
    loop {
        // Read out under a short-lived guard so the second lock below
        // doesn't deadlock against an `if let` temporary.
        let reply = nic::PING_PENDING
            .lock()
            .as_ref()
            .and_then(|p| Some((p.reply_rtt?, p.reply_src?)));
        if let Some((rtt, src)) = reply {
            *nic::PING_PENDING.lock() = None;
            return Ok(Some((rtt, src)));
        }
        if global_timestamp() >= deadline {
            *nic::PING_PENDING.lock() = None;
            return Ok(None);
        }
        sleep(Duration::from_millis(10)).await;
    }
}
