extern crate alloc;

use crate::arp::ArpPacket;
use crate::dns;
use crate::eth::EthernetAddr;
use crate::executor::sleep;
use crate::hpet::global_timestamp;
use crate::icmp;
use crate::ip::IpV4Addr;
use crate::nic;
use crate::nic::PingPending;
use crate::result::Result;
use crate::slice::Sliceable;
use alloc::vec::Vec;
use core::time::Duration;

/// ICMP echo payload length (bytes) used by [`ping_once_result`].
pub const PING_PAYLOAD_LEN: usize = 32;
const PING_REPLY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Find the MAC address a frame for `dst` has to be sent to. That is
/// `dst` itself when it is on our subnet, and the DHCP-learned default
/// router otherwise: only the link-layer next hop changes, the IP packet
/// still targets `dst`. Prods the network with an ARP request if the next
/// hop is not cached yet.
pub async fn next_hop_mac(
    our_mac: EthernetAddr,
    dst: IpV4Addr,
) -> Result<EthernetAddr> {
    const ARP_WAIT: Duration = Duration::from_millis(200);
    let next_hop = nic::next_hop(dst);
    if let Some(mac) = nic::arp_lookup(next_hop) {
        return Ok(mac);
    }
    nic::enqueue_tx_frame(
        ArpPacket::request(our_mac, nic::our_ip(), next_hop)
            .as_slice()
            .to_vec(),
    );
    sleep(ARP_WAIT).await;
    nic::arp_lookup(next_hop).ok_or("ARP unresolved")
}

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
    let dst_mac = next_hop_mac(our_mac, target).await?;

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

/// The DNS server to ask when the caller has no preference: the one the
/// DHCP lease named, or a public resolver when it named none.
pub fn default_dns_server() -> IpV4Addr {
    // 8.8.8.8 is deliberately off our slirp subnet, so the query has to
    // be routed through the DHCP-learned gateway — which also makes
    // `dns` a handy end-to-end test of that routing.
    const DNS_FALLBACK_SERVER: IpV4Addr = IpV4Addr::new([8, 8, 8, 8]);
    nic::dns_server().unwrap_or(DNS_FALLBACK_SERVER)
}

/// Resolve `hostname` to its IPv4 addresses by asking the DNS server at
/// `server`. Named after the POSIX function that applications call to
/// turn a host name into addresses; ours takes only what it needs.
///
/// Returns an `Err` when the NIC is not ready, the server cannot be
/// reached, no reply arrives in time, or the name has no A record.
pub async fn getaddrinfo(
    hostname: &str,
    server: IpV4Addr,
) -> Result<Vec<IpV4Addr>> {
    const DNS_REPLY_TIMEOUT: Duration = Duration::from_millis(2000);
    let our_mac = nic::our_mac().ok_or("NCM not ready (no MAC)")?;
    if !nic::has_ip() {
        return Err("no IP yet (waiting for DHCP)");
    }
    let server_mac = next_hop_mac(our_mac, server).await?;
    let txid: u16 = 0x4321;
    let query = dns::build_query(
        our_mac,
        nic::our_ip(),
        server_mac,
        server,
        hostname,
        txid,
    )?;
    dns::clear_response();
    nic::enqueue_tx_frame(query);
    let deadline = global_timestamp() + DNS_REPLY_TIMEOUT;
    loop {
        if let Some(frame) = dns::take_response() {
            if let Some((rxid, addrs)) = dns::parse_response(&frame) {
                if rxid == txid {
                    if addrs.is_empty() {
                        return Err("no A records");
                    }
                    return Ok(addrs);
                }
            }
        }
        if global_timestamp() >= deadline {
            return Err("timeout waiting for the DNS reply");
        }
        sleep(Duration::from_millis(10)).await;
    }
}
