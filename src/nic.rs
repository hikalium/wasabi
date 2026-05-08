extern crate alloc;

use crate::arp::ArpPacket;
use crate::eth::EthernetAddr;
use crate::executor::spawn_global;
use crate::executor::with_timeout;
use crate::icmp;
use crate::icmp::IcmpPacket;
use crate::info;
use crate::ip::IpV4Addr;
use crate::ip::IpV4Packet;
use crate::ip::IpV4Protocol;
use crate::ncm;
use crate::print::hexdump_bytes;
use crate::result::Result;
use crate::slice::Sliceable;
use crate::usb;
use crate::usb::descriptors_under_config;
use crate::usb::descriptors_under_interface;
use crate::usb::pick_interface_with_triple;
use crate::usb::EndpointDescriptor;
use crate::usb::UsbDescriptor;
use crate::usb::UsbDeviceDescriptor;
use crate::usb::UsbDeviceDriver;
use crate::warn;
use crate::xhci::Controller;
use crate::xhci::EventFuture;
use crate::xhci::NormalTrb;
use crate::xhci::TransferRing;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

const OUR_IP: IpV4Addr = IpV4Addr::new([10, 10, 10, 83]);

// Parse a 12-character upper-case hex MAC string from the device's
// iMacAddress descriptor (CDC ECM 1.2 §5.4) into the 6 raw bytes.
fn parse_mac_hex_str(s: &str) -> Result<EthernetAddr> {
    if s.len() != 12 {
        return Err("MAC: expected 12-char hex string");
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        let chunk = s.get(i * 2..i * 2 + 2).ok_or("MAC: short")?;
        *byte =
            u8::from_str_radix(chunk, 16).map_err(|_| "MAC: bad hex digit")?;
    }
    Ok(EthernetAddr::new(mac))
}

pub struct UsbNcmDriver;
impl UsbNcmDriver {
    pub async fn request_get_net_address(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_ntb_parameters(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 28];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x80,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    pub async fn request_get_network_connection(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
    ) -> Result<Vec<u8>> {
        let buf = vec![0u8; 6];
        let mut buf = Box::into_pin(buf.into_boxed_slice());
        xhc.request_transfer_from_class_interface(
            slot,
            ctrl_ep_ring,
            0x81,
            0,
            0,
            &mut buf,
        )
        .await?;
        Ok(buf.to_vec())
    }
    async fn poll_int_in(
        xhc: Rc<Controller>,
        slot: u8,
        mut ring: TransferRing,
        desc: EndpointDescriptor,
    ) -> Result<()> {
        loop {
            let buf = vec![0u8; 16];
            let mut buf = Box::into_pin(buf.into_boxed_slice());
            let trb_ptr_waiting =
                ring.push(NormalTrb::new_in(&mut buf).into())?;
            let waiter = EventFuture::new_for_trb(
                &xhc.primary_event_ring,
                trb_ptr_waiting,
            );
            xhc.notify_ep(slot, desc.dci())?;

            if let Err(e) = waiter.await.map(|e| e.transfer_result_ok()) {
                info!("failed: {e:?}");
            } else {
                match buf[1] {
                    0x00 => {
                        info!(
                            "Notification: NETWORK_CONNECTION: {}",
                            if buf[2] == 1 {
                                "Connected"
                            } else {
                                "Disconnected"
                            }
                        );
                    }
                    0x2A => {
                        let downlink_bitrate = {
                            let mut v = [0u8; 4];
                            v.copy_from_slice(&buf[8..12]);
                            u32::from_le_bytes(v)
                        };
                        let uplink_bitrate = {
                            let mut v = [0u8; 4];
                            v.copy_from_slice(&buf[12..16]);
                            u32::from_le_bytes(v)
                        };
                        info!(
                            "Notification: CONNECTION_SPEED_CHANGE: \
                            up = {uplink_bitrate} bps, \
                            down = {downlink_bitrate} bps",
                        );
                    }
                    _ => {
                        info!("Notification: ?");
                        hexdump_bytes(&buf);
                    }
                }
            }
        }
    }
    async fn poll_bulk(
        xhc: Rc<Controller>,
        slot: u8,
        mut bulk_in_ring: TransferRing,
        bulk_in_desc: EndpointDescriptor,
        mut bulk_out_ring: TransferRing,
        bulk_out_desc: EndpointDescriptor,
        our_mac: EthernetAddr,
    ) -> Result<()> {
        // Announce ourselves so the host learns our MAC <-> IP binding.
        let arp = ArpPacket::gratuitous(our_mac, OUR_IP);
        Self::send_datagram(
            &xhc,
            slot,
            &mut bulk_out_ring,
            &bulk_out_desc,
            arp.as_slice(),
            0,
        )
        .await?;
        info!("NCM: sent gratuitous ARP for {OUR_IP}");

        let mut tx_seq: u16 = 1;
        loop {
            let buf = vec![0u8; 1024];
            let mut buf = Box::into_pin(buf.into_boxed_slice());
            let trb_ptr_waiting =
                bulk_in_ring.push(NormalTrb::new_in(&mut buf).into())?;

            xhc.notify_ep(slot, bulk_in_desc.dci())?;
            EventFuture::new_for_trb(&xhc.primary_event_ring, trb_ptr_waiting)
                .await?
                .transfer_result_ok()?;
            let nth = match ncm::parse_nth16(&buf) {
                Ok(nth) => nth,
                Err(_) => continue,
            };
            let ntb_len = nth.block_length as usize;
            if ntb_len > buf.len() {
                warn!(
                    "NTB(seq={}): block_length {ntb_len} > buf {}",
                    nth.sequence,
                    buf.len()
                );
                continue;
            }

            // Collect responses first so we don't borrow `buf` across an
            // await (`send_datagram` is async).
            let mut replies: Vec<Vec<u8>> = Vec::new();
            for frame in ncm::iter_ntb16_datagrams(&buf[..ntb_len]) {
                if frame.len() < 14 {
                    continue;
                }
                let eth_type = [frame[12], frame[13]];
                if eth_type == [0x08, 0x06] && frame.len() >= 42 {
                    if let Ok(req) = ArpPacket::copy_from_slice(&frame[..42]) {
                        if req.is_request_for(OUR_IP) {
                            info!(
                                "ARP: request for {} from {} ({:?})",
                                OUR_IP,
                                req.sender_ip(),
                                req.sender_mac(),
                            );
                            replies.push(
                                req.reply_to(our_mac).as_slice().to_vec(),
                            );
                        }
                    }
                } else if eth_type == [0x08, 0x00]
                    && frame.len() >= core::mem::size_of::<IcmpPacket>()
                {
                    if let Ok(ip) = IpV4Packet::copy_from_slice(
                        &frame[..core::mem::size_of::<IpV4Packet>()],
                    ) {
                        if ip.dst() == OUR_IP
                            && ip.protocol() == IpV4Protocol::icmp()
                        {
                            // Trim to ip.total_length() to drop any
                            // Ethernet-layer padding (frames < 60 bytes).
                            let frame_total = core::mem::size_of::<
                                crate::eth::EthernetHeader,
                            >() + ip.total_length();
                            let frame_total = frame_total.min(frame.len());
                            match icmp::echo_reply_from_request(
                                &frame[..frame_total],
                                our_mac,
                                OUR_IP,
                            ) {
                                Ok(reply) => {
                                    info!(
                                        "ICMP: echo request from {} -> reply",
                                        ip.src(),
                                    );
                                    replies.push(reply);
                                }
                                Err(e) => warn!("ICMP reply build: {e}"),
                            }
                        }
                    }
                }
            }
            for reply in replies {
                Self::send_datagram(
                    &xhc,
                    slot,
                    &mut bulk_out_ring,
                    &bulk_out_desc,
                    &reply,
                    tx_seq,
                )
                .await?;
                tx_seq = tx_seq.wrapping_add(1);
            }
        }
    }
    async fn send_datagram(
        xhc: &Rc<Controller>,
        slot: u8,
        ring: &mut TransferRing,
        desc: &EndpointDescriptor,
        datagram: &[u8],
        seq: u16,
    ) -> Result<()> {
        let ntb = ncm::build_ntb16(datagram, seq);
        let buf = Box::into_pin(ntb.into_boxed_slice());
        let trb_ptr_waiting = ring.push(NormalTrb::new_out(&buf).into())?;
        xhc.notify_ep(slot, desc.dci())?;
        EventFuture::new_for_trb(&xhc.primary_event_ring, trb_ptr_waiting)
            .await?
            .transfer_result_ok()?;
        Ok(())
    }
    async fn run(
        xhc: &Rc<Controller>,
        port: usize,
        slot: u8,
        ctrl_ep_ring: &mut TransferRing,
        descriptors: &[UsbDescriptor],
    ) -> Result<()> {
        /*
        interface 0 alt 0 02:0D:00
                02: Communication Interface Class
                0D: Network Control Model Subclass
                00: Protocol defined in the USB Spec
            EP 1 interrupt in mps 16 interval 11
                SSEC 0, 0, 8, 0
        interface 1 alt 0 0A:00:01
        interface 1 alt 1 0A:00:01
            EP 2 bulk in mps 0x400 interval 0
                SSEC 5, 0, 0, 0
            EP 3 bulk out mps 0x400 interval 0
                SSEC 5, 0, 0, 0
        */
        let (config_desc, _, _) =
            pick_interface_with_triple(descriptors, (2, 13, 0))
                .ok_or("No USB NCM Communications interface found")?;
        info!("C: {config_desc:?}");
        let desc_under_config =
            descriptors_under_config(descriptors, config_desc.config_value());
        let mut mac_addr_index = 0;
        for d in &desc_under_config {
            if let UsbDescriptor::Interface(e) = d {
                info!("I:   {e:?}")
            } else if let UsbDescriptor::Endpoint(e) = d {
                info!("E:     {e:?}")
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x30,
                payload,
                ..
            } = d
            {
                info!("SSEC:    {payload:?}")
            } else if let UsbDescriptor::Unknown {
                desc_type: 0x24, /* CS_INTERFACE [ncm_1_1] Table 6-2 */
                payload,
                ..
            } = d
            {
                let subtype = payload.first().cloned().unwrap_or_default();
                match subtype {
                    0x0F => {
                        /* Ethernet Networking Functional Descriptor [cdc_1_2
                         * Table 13] */
                        // Expected to be non-zero.
                        mac_addr_index =
                            payload.get(1).cloned().unwrap_or_default();
                    }
                    _ => {
                        info!("?   :    {d:?}")
                    }
                }
            } else if let UsbDescriptor::Unknown { .. } = d {
                info!("?   :    {d:?}")
            }
        }

        let mac_addr = {
            let res = with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor_zero(xhc, slot, ctrl_ep_ring),
            )
            .await?;
            // If there is one lang_id, bLength will be 4
            if res[0] < 4 {
                return Err("string desc zero too short");
            }
            let lang_id = u16::from_le_bytes([res[2], res[3]]);
            with_timeout(
                Duration::from_secs(1),
                usb::request_string_descriptor(
                    xhc,
                    slot,
                    ctrl_ep_ring,
                    lang_id,
                    mac_addr_index,
                ),
            )
            .await?
        };
        info!("iMacAddress: {mac_addr:?}");

        //
        // Set up communications interface
        //

        let int_in_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 0, 0);
            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if d.is_dir_in() && d.is_interrupt_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("interrupt_in_ep_desc not found")
        }?;

        let bulk_in_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 1, 1);

            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if d.is_dir_in() && d.is_bulk_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("bulk_in_ep_desc not found")
        }?;

        let bulk_out_ep_desc = {
            let desc_under_com_interface =
                descriptors_under_interface(&desc_under_config, 1, 1);

            desc_under_com_interface
                .iter()
                .find_map(|d| {
                    if let usb::UsbDescriptor::Endpoint(d) = d {
                        if !d.is_dir_in() && d.is_bulk_endpoint() {
                            return Some(d);
                        }
                    }
                    None
                })
                .cloned()
                .ok_or("bulk_out_ep_desc not found")
        }?;

        let mut ring_list = usb::configure_endpoint(
            xhc,
            port,
            slot,
            &[int_in_ep_desc, bulk_in_ep_desc, bulk_out_ep_desc],
        )
        .await?;
        let int_in_ep_ring = ring_list
            .remove(&int_in_ep_desc.dci())
            .ok_or("ep_ring for interrupt in was not populated")?;
        let bulk_in_ep_ring = ring_list
            .remove(&bulk_in_ep_desc.dci())
            .ok_or("ep_ring for bulk in was not populated")?;
        let bulk_out_ep_ring = ring_list
            .remove(&bulk_out_ep_desc.dci())
            .ok_or("ep_ring for bulk out was not populated")?;

        xhc.request_set_config(slot, ctrl_ep_ring, 2).await?;
        xhc.request_set_interface(slot, ctrl_ep_ring, 0, 0).await?;
        // start operation!
        xhc.request_set_interface(slot, ctrl_ep_ring, 1, 1).await?;

        let ntbparams =
            Self::request_get_ntb_parameters(xhc, slot, ctrl_ep_ring).await?;
        info!("ntbparams: {ntbparams:?}");

        let our_mac = parse_mac_hex_str(&mac_addr)?;
        info!("NCM: our MAC = {our_mac:?}, IP = {OUR_IP}");

        spawn_global(Self::poll_int_in(
            xhc.clone(),
            slot,
            int_in_ep_ring,
            int_in_ep_desc,
        ));
        spawn_global(Self::poll_bulk(
            xhc.clone(),
            slot,
            bulk_in_ep_ring,
            bulk_in_ep_desc,
            bulk_out_ep_ring,
            bulk_out_ep_desc,
            our_mac,
        ));
        Ok(())
    }
}
impl UsbDeviceDriver for UsbNcmDriver {
    fn is_compatible(
        &self,
        descriptors: &[UsbDescriptor],
        _device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        // Note: QEMU's usb-nic does not have this interface.
        pick_interface_with_triple(
            descriptors,
            (
                2,  /* Communications Interface Class [cdc_1_2] 4.2 */
                13, /* Network Control Model [cdc_1_2] 4.3 */
                0,
            ),
        )
        .is_some()
    }
    fn start(
        &self,
        xhc: Rc<Controller>,
        port: usize,
        slot: u8,
        mut ctrl_ep_ring: TransferRing,
        descriptors: Vec<UsbDescriptor>,
        _device_descriptor: &UsbDeviceDescriptor,
    ) {
        spawn_global(async move {
            Self::run(&xhc, port, slot, &mut ctrl_ep_ring, &descriptors).await
        });
    }
}
