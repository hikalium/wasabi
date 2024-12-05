extern crate alloc;

use crate::bits::extract_bits;
use crate::bits::extract_bits_from_le_bytes;
use crate::executor::spawn_global;
use crate::info;
use crate::print::get_global_vram_resolutions;
use crate::print::hexdump_bytes;
use crate::range::map_value_in_range_inclusive;
use crate::result::Result;
use crate::usb::*;
use crate::warn;
use crate::xhci::CommandRing;
use crate::xhci::Controller;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::RangeInclusive;

#[derive(Debug)]
#[repr(u8)]
#[allow(dead_code)]
enum UsbHidReportItemType {
    Main = 0,
    Global = 1,
    Local = 2,
    Reserved = 3,
}

#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
pub enum UsbHidUsagePage {
    GenericDesktop,
    Button,
    UnknownUsagePage(usize),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UsbHidUsage {
    Pointer,
    Mouse,
    X,
    Y,
    Wheel,
    Button(usize),
    UnknownUsage(usize),
    Constant,
}

fn parse_hid_report_descriptor(report: &[u8]) -> Result<Vec<UsbHidReportInputItem>> {
    let mut it = report.iter();
    let mut input_report_items = Vec::new();
    let mut usage_queue = VecDeque::new();
    let mut usage_page: Option<UsbHidUsagePage> = None;
    let mut usage_min = None;
    let mut usage_max = None;
    let mut report_size = 0;
    let mut report_count = 0;
    let mut bit_offset = 0;
    let mut logical_min = 0;
    let mut logical_max = 0;
    while let Some(prefix) = it.next() {
        let b_size = match prefix & 0b11 {
            0b11 => 4,
            e => e,
        } as usize;
        let b_type = match (prefix >> 2) & 0b11 {
            0 => UsbHidReportItemType::Main,
            1 => UsbHidReportItemType::Global,
            2 => UsbHidReportItemType::Local,
            _ => {
                warn!("b_type == Reserved is not implemented yet!");
                break;
            }
        };
        let b_tag = prefix >> 4;
        let data: Vec<u8> = it.by_ref().take(b_size).cloned().collect();
        let data_value = {
            let mut data = data.clone();
            data.resize(4, 0u8);
            let mut value = [0u8; 4];
            value.copy_from_slice(&data);
            u32::from_le_bytes(value)
        };
        match (&b_type, &b_tag) {
            (UsbHidReportItemType::Main, 0b1000) => {
                info!("M: Input attr {data_value:#b}");
                if let Some(usage_page) = usage_page {
                    let is_constant = extract_bits(data_value, 0, 1) == 1;
                    let is_array = extract_bits(data_value, 1, 1) == 1;
                    let is_absolute = extract_bits(data_value, 2, 1) == 0;
                    for i in 0..report_count {
                        let report_usage = if let Some(usage) = usage_queue.pop_front() {
                            usage
                        } else if let (UsbHidUsagePage::Button, Some(usage_min), Some(usage_max)) =
                            (usage_page, usage_min, usage_max)
                        {
                            let btn_idx = usage_min + i;
                            if btn_idx <= usage_max {
                                UsbHidUsage::Button(btn_idx)
                            } else {
                                UsbHidUsage::UnknownUsage(btn_idx)
                            }
                        } else if is_constant {
                            UsbHidUsage::Constant
                        } else {
                            UsbHidUsage::UnknownUsage(0)
                        };
                        input_report_items.push(UsbHidReportInputItem {
                            usage: report_usage,
                            bit_size: report_size,
                            is_array,
                            is_absolute,
                            bit_offset,
                            logical_min,
                            logical_max,
                        });
                        bit_offset += report_size;
                    }
                }
            }
            (UsbHidReportItemType::Main, 0b1010) => {
                let collection_type = match data_value {
                    0 => "Physical".to_string(),
                    1 => "Application".to_string(),
                    v => format!("{v}"),
                };
                info!("M: Collection {collection_type} {{",)
            }
            (UsbHidReportItemType::Main, 0b1100) => {
                info!("M: }} Collection",)
            }
            (UsbHidReportItemType::Global, 0b0000) => {
                usage_page = Some(match data_value {
                    0x01 => UsbHidUsagePage::GenericDesktop,
                    0x09 => UsbHidUsagePage::Button,
                    _ => UsbHidUsagePage::UnknownUsagePage(data_value as usize),
                });
                info!("G: Usage Page: {usage_page:?}",);
            }
            (UsbHidReportItemType::Global, 0b0001) => {
                info!("G: Logical Minimum: {data_value:#X}");
                logical_min = data_value;
            }
            (UsbHidReportItemType::Global, 0b0010) => {
                info!("G: Logical Maximum: {data_value:#X}");
                logical_max = data_value;
            }
            (UsbHidReportItemType::Global, 0b0111) => {
                info!("G: Report Size: {data_value} bits");
                report_size = data_value as usize;
            }
            (UsbHidReportItemType::Global, 0b1001) => {
                info!("G: Report Count: {data_value} times");
                report_count = data_value as usize;
            }
            (UsbHidReportItemType::Local, 0) => {
                let usage = match &usage_page {
                    Some(UsbHidUsagePage::GenericDesktop) => match data_value {
                        0x01 => UsbHidUsage::Pointer,
                        0x02 => UsbHidUsage::Mouse,
                        0x30 => UsbHidUsage::X,
                        0x31 => UsbHidUsage::Y,
                        0x38 => UsbHidUsage::Wheel,
                        _ => UsbHidUsage::UnknownUsage(data_value as usize),
                    },
                    _ => UsbHidUsage::UnknownUsage(data_value as usize),
                };
                usage_queue.push_back(usage);
                info!(
                    "L: Usage: {usage:?} (in usage page {})",
                    format!("{usage_page:#X?}")
                        .replace('\n', "")
                        .replace(' ', "")
                )
            }
            (UsbHidReportItemType::Local, 1) => usage_min = Some(data_value as usize),
            (UsbHidReportItemType::Local, 2) => usage_max = Some(data_value as usize),
            _ => {
                info!(
                    "{prefix:#04X} (type = {:6}, tag = {b_tag:2}): {}",
                    format!("{b_type:?}"),
                    format!("{data:#04X?}").replace('\n', "").replace(' ', "")
                );
            }
        }
        if matches!(b_type, UsbHidReportItemType::Main) {
            usage_queue.clear();
            usage_min = None;
            usage_max = None;
        }
    }
    Ok(input_report_items)
}

#[derive(Debug)]
pub struct UsbHidReportInputItem {
    pub usage: UsbHidUsage,
    pub bit_size: usize,
    pub is_array: bool,
    pub is_absolute: bool,
    pub bit_offset: usize,
    pub logical_min: u32,
    pub logical_max: u32,
}
impl UsbHidReportInputItem {
    fn value_from_report(&self, report: &[u8]) -> Option<i64> {
        extract_bits_from_le_bytes(report, self.bit_offset, self.bit_size).map(|v| {
            if self.bit_size >= 2 && extract_bits(v, self.bit_size - 1, 1) == 1 {
                -(!extract_bits(v, 0, self.bit_size - 1) as i64) - 1
            } else {
                v as i64
            }
        })
    }
    fn mapped_range_from_report(
        &self,
        report: &[u8],
        to_range: RangeInclusive<i64>,
    ) -> Result<i64> {
        let v = self.value_from_report(report).ok_or("value was empty")?;
        map_value_in_range_inclusive(
            (self.logical_min as i64)..=(self.logical_max as i64),
            to_range,
            v,
        )
    }
}

pub struct UsbTabletDriver;
impl UsbTabletDriver {
    async fn run(
        xhc: &Rc<Controller>,
        slot: u8,
        ctrl_ep_ring: &mut CommandRing,
        descriptors: &[UsbDescriptor],
    ) -> Result<()> {
        let (_config_desc, interface_desc, other_desc_list) =
            pick_interface_with_triple(descriptors, (3, 0, 0))
                .ok_or("No USB KBD Boot interface found")?;
        info!("USB tablet found");
        let hid_desc = other_desc_list
            .iter()
            .flat_map(|e| match e {
                UsbDescriptor::Hid(e) => Some(e),
                _ => None,
            })
            .next()
            .ok_or("No HID Descriptor found")?;
        info!("HID Descriptor: {hid_desc:?}");
        let report = request_hid_report_descriptor(
            xhc,
            slot,
            ctrl_ep_ring,
            interface_desc.interface_number,
            hid_desc.report_descriptor_length as usize,
        )
        .await?;
        info!("Report Descriptor:");
        hexdump_bytes(&report);
        let input_report_items = parse_hid_report_descriptor(&report)?;
        for e in &input_report_items {
            info!("  {e:?}")
        }
        let report_size_in_byte = if let Some(last_item) = input_report_items.last() {
            (last_item.bit_offset + last_item.bit_size + 7) / 8
        } else {
            return Err("report size is zero");
        };
        let mut prev_report = vec![0u8; report_size_in_byte];
        let desc_button_l = input_report_items
            .iter()
            .find(|e| e.usage == UsbHidUsage::Button(1))
            .ok_or("Button(1) not found")?;
        let desc_button_r = input_report_items
            .iter()
            .find(|e| e.usage == UsbHidUsage::Button(2))
            .ok_or("Button(2) not found")?;
        let desc_button_c = input_report_items
            .iter()
            .find(|e| e.usage == UsbHidUsage::Button(3))
            .ok_or("Button(3) not found")?;
        let desc_abs_x = input_report_items
            .iter()
            .find(|e| e.usage == UsbHidUsage::X && e.is_absolute)
            .ok_or("Absolute pointer X not found")?;
        let desc_abs_y = input_report_items
            .iter()
            .find(|e| e.usage == UsbHidUsage::Y && e.is_absolute)
            .ok_or("Absolute pointer Y not found")?;

        let (vw, vh) = get_global_vram_resolutions().ok_or("global VRAM is not set")?;
        loop {
            let report = request_hid_report(xhc, slot, ctrl_ep_ring).await?;
            if report == prev_report {
                continue;
            }
            let l = desc_button_l.value_from_report(&report);
            let r = desc_button_r.value_from_report(&report);
            let c = desc_button_c.value_from_report(&report);
            let ax = desc_abs_x.mapped_range_from_report(&report, 0..=(vw - 1));
            let ay = desc_abs_y.mapped_range_from_report(&report, 0..=(vh - 1));
            info!("{report:?}: ({l:?}, {c:?}, {r:?}, {ax:?}, {ay:?})");
            prev_report = report;
        }
    }
}
impl UsbDeviceDriver for UsbTabletDriver {
    fn is_compatible(
        descriptors: &[UsbDescriptor],
        device_descriptor: &UsbDeviceDescriptor,
    ) -> bool {
        device_descriptor.device_class == 0
            && device_descriptor.device_subclass == 0
            && device_descriptor.device_protocol == 0
            && device_descriptor.vendor_id == 0x0627
            && device_descriptor.product_id == 0x0001
            && pick_interface_with_triple(descriptors, (3, 0, 0)).is_some()
    }
    fn start(
        xhc: Rc<Controller>,
        slot: u8,
        mut ctrl_ep_ring: CommandRing,
        descriptors: Vec<UsbDescriptor>,
    ) {
        spawn_global(async move { Self::run(&xhc, slot, &mut ctrl_ep_ring, &descriptors).await });
    }
}
