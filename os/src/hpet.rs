use crate::println;
use core::fmt;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

const TIMER_CONFIG_INT_ENABLE: u64 = 1 << 2;
const TIMER_CONFIG_USE_PERIODIC_MODE: u64 = 1 << 3;
const TIMER_CONFIG_SET_COMPARATOR_VALUE: u64 = 1 << 6;

const GENERAL_CONFIG_ENABLE: u64 = 1 << 0;

#[repr(C)]
struct TimerRegister {
    configuration_and_capability: u64,
    comparator_value: u64,
    _reserved: [u64; 2],
}
impl TimerRegister {
    fn available_interrupt_routes(&self) -> u32 {
        unsafe { (read_volatile(&self.configuration_and_capability) >> 32) as u32 }
    }
    fn disable(&mut self) {
        unsafe {
            write_volatile(&mut self.configuration_and_capability, 0);
        }
    }
    /// # Safety
    /// This is safe only when HPET is globally disabled.
    unsafe fn setup(&mut self, comparator_value: u64, gsi: u64) {
        self.disable();
        write_volatile(&mut self.comparator_value, 0);
        let mut config = 0;
        config |= TIMER_CONFIG_SET_COMPARATOR_VALUE;
        write_volatile(&mut self.configuration_and_capability, config);
        write_volatile(&mut self.comparator_value, comparator_value);
        config |= TIMER_CONFIG_USE_PERIODIC_MODE;
        config |= gsi << 9;
        config |= TIMER_CONFIG_INT_ENABLE;
        write_volatile(&mut self.configuration_and_capability, config);
    }
}
impl fmt::Debug for TimerRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "TimerRegister{{ available_interrupts: {:#b} }}",
            self.available_interrupt_routes()
        )
    }
}

#[repr(C)]
pub struct Registers {
    capabilities_and_id: u64,
    _reserved0: u64,
    configuration: u64,
    _reserved1: u64,
    interrupt_status: u64,
    _reserved2: [u64; 25],
    main_counter_value: u64,
    reserved3: u64,
    timers: [TimerRegister; 32],
}

pub struct Hpet {
    registers: &'static mut Registers,
    #[allow(unused)]
    fs_per_count: u64,
    num_of_timers: usize,
    freq: u64,
}
impl Hpet {
    /// # Safety
    /// Do not call this function twice since it invalidates the previous one.
    pub unsafe fn new(registers: &'static mut Registers) -> Self {
        let fs_per_count = registers.capabilities_and_id >> 32;
        let num_of_timers = ((registers.capabilities_and_id >> 8) & 0b11111) as usize + 1;
        let freq = 1_000_000_000_000_000 / fs_per_count;
        println!(
            "HPET@{:#p}, freq = {}, {} timers",
            registers, freq, num_of_timers
        );

        let mut hpet = Self {
            registers,
            fs_per_count,
            num_of_timers,
            freq,
        };
        hpet.init();
        hpet
    }
    unsafe fn globally_disable(&mut self) {
        let config = read_volatile(&self.registers.configuration) & !GENERAL_CONFIG_ENABLE;
        write_volatile(&mut self.registers.configuration, config);
    }
    unsafe fn globally_enable(&mut self) {
        let config = read_volatile(&self.registers.configuration) | GENERAL_CONFIG_ENABLE;
        write_volatile(&mut self.registers.configuration, config);
    }
    unsafe fn init(&mut self) {
        // c.f. 2.3.9.2.2 Periodic Mode
        self.globally_disable();
        write_volatile(&mut self.registers.main_counter_value, 0);
        for i in 0..self.num_of_timers {
            self.registers.timers[i].disable();
            println!("{:?}", self.registers.timers[i]);
        }
        // Check if timer[0] can cause system interrupt[2]
        assert!(self.registers.timers[0].available_interrupt_routes() & (1 << 2) != 0);
        self.registers.timers[0].setup(self.freq, 2);
        self.globally_enable();
    }
    pub fn main_counter(&self) -> u64 {
        // This is safe as far as self is properly constructed.
        unsafe { read_volatile(&self.registers.main_counter_value) }
    }
}
