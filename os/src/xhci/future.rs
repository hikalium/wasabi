extern crate alloc;

use crate::error::Result;
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

#[derive(Clone)]
pub struct EventFuture {
    wait_on: Rc<EventWaitInfo>,
    _pinned: PhantomPinned,
}
impl EventFuture {
    pub fn new(event_ring: &Mutex<EventRing>, cond: EventWaitCond) -> Self {
        let wait_on = EventWaitInfo {
            cond,
            trbs: Default::default(),
        };
        let wait_on = Rc::new(wait_on);
        event_ring.lock().register_waiter(&wait_on);
        Self {
            wait_on,
            _pinned: PhantomPinned,
        }
    }
    pub fn new_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new(
            event_ring,
            EventWaitCond {
                trb_type: None,
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_command_completion_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new(
            event_ring,
            EventWaitCond {
                trb_type: Some(TrbType::CommandCompletionEvent),
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_transfer_event_on_slot(event_ring: &Mutex<EventRing>, slot: u8) -> Self {
        Self::new(
            event_ring,
            EventWaitCond {
                trb_type: Some(TrbType::TransferEvent),
                trb_addr: None,
                slot: Some(slot),
            },
        )
    }
    pub fn new_on_trb(event_ring: &Mutex<EventRing>, trb_addr: u64) -> Self {
        Self::new(
            event_ring,
            EventWaitCond {
                trb_type: None,
                trb_addr: Some(trb_addr),
                slot: None,
            },
        )
    }
}
/// Event
impl Future for EventFuture {
    type Output = Result<GenericTrbEntry>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<GenericTrbEntry>> {
        let mut_self = unsafe { self.get_unchecked_mut() };
        if let Some(trb) = mut_self.wait_on.trbs.lock().pop_front() {
            Poll::Ready(Ok(trb))
        } else {
            Poll::Pending
        }
    }
}
