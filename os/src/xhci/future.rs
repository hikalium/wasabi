extern crate alloc;

use crate::error::Result;
use crate::hpet::Hpet;
use crate::mutex::Mutex;
use crate::xhci::ring::EventRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::trb::TrbType;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use core::future::Future;
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;

#[derive(Debug)]
pub struct EventWaitCond {
    trb_type: Option<TrbType>,
    trb_addr: Option<u64>,
    slot: Option<u8>,
}

#[derive(Debug)]
pub struct EventWaitInfo {
    cond: EventWaitCond,
    trbs: Mutex<VecDeque<GenericTrbEntry>>,
}
impl EventWaitInfo {
    pub fn matches(&self, trb: &GenericTrbEntry) -> bool {
        if let Some(trb_type) = self.cond.trb_type {
            if trb.trb_type() != trb_type as u32 {
                return false;
            }
        }
        if let Some(slot) = self.cond.slot {
            if trb.slot_id() != slot {
                return false;
            }
        }
        if let Some(trb_addr) = self.cond.trb_addr {
            if trb.data() != trb_addr {
                return false;
            }
        }
        true
    }
    pub fn resolve(&self, trb: &GenericTrbEntry) -> Result<()> {
        self.trbs.under_locked(&|trbs| -> Result<()> {
            trbs.push_back(trb.clone());
            Ok(())
        })
    }
}

pub enum EventFutureWaitType {
    TrbAddr(u64),
    Slot(u8),
}

pub struct EventFuture {
    wait_on: Rc<EventWaitInfo>,
    time_out: u64,
    _pinned: PhantomPinned,
}
impl EventFuture {
    pub fn new_with_timeout(
        event_ring: &Mutex<EventRing>,
        wait_ms: u64,
        cond: EventWaitCond,
    ) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 1000 * wait_ms;
        let wait_on = EventWaitInfo {
            cond,
            trbs: Default::default(),
        };
        let wait_on = Rc::new(wait_on);
        event_ring.lock().register_waiter(&wait_on);
        Self {
            wait_on,
            time_out,
            _pinned: PhantomPinned,
        }
    }
    pub fn new_on_slot_with_timeout(event_ring: &Mutex<EventRing>, slot: u8, wait_ms: u64) -> Self {
        Self::new_with_timeout(
            event_ring,
            wait_ms,
            EventWaitCond {
                trb_type: None,
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_command_completion_on_slot_with_timeout(
        event_ring: &Mutex<EventRing>,
        slot: u8,
        wait_ms: u64,
    ) -> Self {
        Self::new_with_timeout(
            event_ring,
            wait_ms,
            EventWaitCond {
                trb_type: Some(TrbType::CommandCompletionEvent),
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_command_completion_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new_with_timeout(
            event_ring,
            100,
            EventWaitCond {
                trb_type: Some(TrbType::CommandCompletionEvent),
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_transfer_event_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new_with_timeout(
            event_ring,
            100,
            EventWaitCond {
                trb_type: Some(TrbType::TransferEvent),
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new_on_slot_with_timeout(event_ring, slot, 100)
    }
    pub fn new_on_trb_with_timeout(
        event_ring: &Mutex<EventRing>,
        trb_addr: u64,
        wait_ms: u64,
    ) -> Self {
        Self::new_with_timeout(
            event_ring,
            wait_ms,
            EventWaitCond {
                trb_type: None,
                trb_addr: Some(trb_addr),
                slot: None,
            },
        )
    }
    pub fn new_on_trb(event_ring: &Mutex<EventRing>, trb_addr: u64) -> Self {
        Self::new_on_trb_with_timeout(event_ring, trb_addr, 100)
    }
}
/// Event
impl Future for EventFuture {
    type Output = Result<VecDeque<GenericTrbEntry>>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<VecDeque<GenericTrbEntry>>> {
        if self.time_out < Hpet::take().main_counter() {
            return Poll::Ready(Ok(Default::default()));
        }
        let mut_self = unsafe { self.get_unchecked_mut() };
        if mut_self.wait_on.trbs.lock().len() > 0 {
            Poll::Ready(Ok(mut_self.wait_on.trbs.lock().clone()))
        } else {
            Poll::Pending
        }
    }
}
