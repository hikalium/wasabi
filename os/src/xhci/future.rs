use crate::error::Result;
use crate::hpet::Hpet;
use crate::mutex::Mutex;
use crate::println;
use crate::xhci::ring::EventRing;
use crate::xhci::trb::GenericTrbEntry;
use crate::xhci::trb::TrbType;
use core::future::Future;
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;

pub enum EventFutureWaitType {
    TrbAddr(u64),
    Slot(u8),
}

pub struct EventFuture<'a, const E: TrbType> {
    event_ring: &'a Mutex<EventRing>,
    wait_on: EventFutureWaitType,
    time_out: u64,
    _pinned: PhantomPinned,
}
impl<'a, const E: TrbType> EventFuture<'a, E> {
    pub fn new(event_ring: &'a Mutex<EventRing>, trb_addr: u64) -> Self {
        Self::new_with_timeout(event_ring, trb_addr, 100)
    }
    pub fn new_with_timeout(event_ring: &'a Mutex<EventRing>, trb_addr: u64, wait_ms: u64) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 1000 * wait_ms;
        Self {
            event_ring,
            wait_on: EventFutureWaitType::TrbAddr(trb_addr),
            time_out,
            _pinned: PhantomPinned,
        }
    }
    pub fn new_on_slot(event_ring: &'a Mutex<EventRing>, slot: u8) -> Self {
        let time_out = Hpet::take().main_counter() + Hpet::take().freq() / 10;
        Self {
            event_ring,
            wait_on: EventFutureWaitType::Slot(slot),
            time_out,
            _pinned: PhantomPinned,
        }
    }
}
impl<'a, const E: TrbType> Future for EventFuture<'a, E> {
    type Output = Result<Option<GenericTrbEntry>>;
    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<Option<GenericTrbEntry>>> {
        let time_out = self.time_out;
        let mut_self = unsafe { self.get_unchecked_mut() };
        let event = mut_self.event_ring.lock().pop();
        match event {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => {
                if time_out < Hpet::take().main_counter() {
                    Poll::Ready(Ok(None))
                } else {
                    Poll::Pending
                }
            }
            Ok(Some(trb)) => {
                if trb.trb_type() != E as u32 {
                    println!("Ignoring event (!= type): {:?}", trb);
                    return Poll::Pending;
                }
                match mut_self.wait_on {
                    EventFutureWaitType::TrbAddr(trb_addr) if trb_addr == trb.data() => {
                        Poll::Ready(Ok(Some(trb)))
                    }
                    EventFutureWaitType::Slot(slot) if trb.slot_id() == slot => {
                        Poll::Ready(Ok(Some(trb)))
                    }
                    _ => {
                        println!("Ignoring event (!= wait_on): {:?}", trb);
                        Poll::Pending
                    }
                }
            }
        }
    }
}
pub type CommandCompletionEventFuture<'a> = EventFuture<'a, { TrbType::CommandCompletionEvent }>;
pub type TransferEventFuture<'a> = EventFuture<'a, { TrbType::TransferEvent }>;
