extern crate alloc;

use crate::error::Result;
use crate::hpet::Hpet;
use crate::mutex::Mutex;
use crate::xhci::ring::EventRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::trb::TrbType;
use alloc::collections::VecDeque;
use core::future::Future;
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;

pub struct EventWaitInfo {
    trb_type: TrbType,
    trb_addr: Option<u64>,
    slot: Option<u8>,
    fullfilled: AtomicBool,
    event_trb: Mutex<GenericTrbEntry>,
}

static EVENT_QUEUE: Mutex<VecDeque<GenericTrbEntry>> = Mutex::new(VecDeque::new());

pub enum EventFutureWaitType {
    TrbAddr(u64),
    Slot(u8),
}

pub struct EventFuture<const E: TrbType> {
    wait_on: EventWaitInfo,
    time_out: u64,
    _pinned: PhantomPinned,
}
impl<'a, const E: TrbType> EventFuture<E> {
    pub fn new_with_timeout(
        event_ring: &Mutex<EventRing>,
        wait_ms: u64,
        wait_on: EventFutureWaitType,
    ) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 1000 * wait_ms;
        /*
        match mut_self.wait_on {
            EventFutureWaitType::Slot(slot) => {
                let event = mut_self.event_ring.lock().pop_for_slot(slot);
                if let Ok(Some(_)) = event {
                    Poll::Ready(event)
                } else {
                    Poll::Pending
                }
            }
            EventFutureWaitType::TrbAddr(trb_addr) => {
                let event = mut_self.event_ring.lock().pop_for_trb(trb_addr);
                if let Ok(Some(_)) = event {
                    Poll::Ready(event)
                } else {
                    Poll::Pending
                }
            }
        }
        */
        let wait_on = EventWaitInfo {
            trb_type: E,
            trb_addr: None,
            slot: None,
            fullfilled: Default::default(),
            event_trb: Default::default(),
        };
        Self {
            wait_on,
            time_out,
            _pinned: PhantomPinned,
        }
    }
    pub fn new_on_slot_with_timeout(
        event_ring: &'a Mutex<EventRing>,
        slot: u8,
        wait_ms: u64,
    ) -> Self {
        Self::new_with_timeout(event_ring, wait_ms, EventFutureWaitType::Slot(slot))
    }
    pub fn new_on_slot(event_ring: &'a Mutex<EventRing>, slot: u8) -> Self {
        Self::new_on_slot_with_timeout(event_ring, slot, 100)
    }
    pub fn new_on_trb_with_timeout(
        event_ring: &'a Mutex<EventRing>,
        trb_addr: u64,
        wait_ms: u64,
    ) -> Self {
        Self::new_with_timeout(event_ring, wait_ms, EventFutureWaitType::TrbAddr(trb_addr))
    }
    pub fn new_on_trb(event_ring: &'a Mutex<EventRing>, trb_addr: u64) -> Self {
        Self::new_on_trb_with_timeout(event_ring, trb_addr, 100)
    }
}
/// Event
impl<'a, const E: TrbType> Future for EventFuture<E> {
    type Output = Result<Option<GenericTrbEntry>>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<Option<GenericTrbEntry>>> {
        if self.time_out < Hpet::take().main_counter() {
            return Poll::Ready(Ok(None));
        }
        let mut_self = unsafe { self.get_unchecked_mut() };
        if mut_self.wait_on.fullfilled.load(Ordering::SeqCst) {
            Poll::Ready(Ok(Some((*mut_self.wait_on.event_trb.lock()).clone())))
        } else {
            Poll::Pending
        }
    }
}
pub type CommandCompletionEventFuture<'a> = EventFuture<{ TrbType::CommandCompletionEvent }>;
pub type TransferEventFuture<'a> = EventFuture<{ TrbType::TransferEvent }>;
