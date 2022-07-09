use crate::println;
use core::fmt;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

const TIMER_CONFIG_LEVEL_TRIGGER: u64 = 1 << 1;
const TIMER_CONFIG_INT_ENABLE: u64 = 1 << 2;
const TIMER_CONFIG_USE_PERIODIC_MODE: u64 = 1 << 3;
const TIMER_CONFIG_SET_COMPARATOR_VALUE: u64 = 1 << 6;

#[repr(C)]
struct TimerRegister {
    configuration_and_capability: u64,
    comparator_value: u64,
    _reserved: [u64; 2],
}
const _: () = assert!(core::mem::size_of::<TimerRegister>() == 0x20);
impl TimerRegister {
    fn available_interrupt_routes(&self) -> u32 {
        unsafe { (read_volatile(&self.configuration_and_capability) >> 32) as u32 }
    }
    unsafe fn write_config(&mut self, config: u64) {
        println!("write_config {:#018X}", config);
        write_volatile(&mut self.configuration_and_capability, config);
    }
}
impl fmt::Debug for TimerRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TimerRegister{{ available_interrupts: {:#b}, config_and_capability: {:#b}, comparator: {}}}",
            self.available_interrupt_routes(),
            self.configuration_and_capability,
            self.comparator_value
        )
    }
}

#[repr(C)]
pub struct Registers {
    capabilities_and_id: u64,
    _reserved0: u64,
    configuration: u64,
    _reserved1: u64,
    interrupt_status: AtomicU64,
    _reserved12: u64,
    _reserved2: [u64; 24],
    main_counter_value: u64,
    reserved3: u64,
    timers: [TimerRegister; 32],
}
const _: () = assert!(core::mem::size_of::<Registers>() == 0x500);

pub struct Hpet {
    registers: &'static mut Registers,
    #[allow(unused)]
    fs_per_count: u64,
    num_of_timers: usize,
    freq: u64,
}
static mut HPET: Option<Hpet> = None;
impl Hpet {
    pub fn take() -> &'static mut Self {
        unsafe { HPET.as_mut().expect("HPET is not initialized") }
    }
    /// # Safety
    /// This is safe if it is called only once.
    pub unsafe fn set(hpet: Hpet) {
        assert!(HPET.is_none());
        HPET = Some(hpet);
        println!("HPET populated!");
    }
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
        let config = read_volatile(&self.registers.configuration) & !0b11;
        println!("general_config set: {:#b}", config);
        write_volatile(&mut self.registers.configuration, config);
        println!(
            "general_config set: {:#b}",
            read_volatile(&self.registers.configuration)
        );
    }
    unsafe fn globally_enable(&mut self) {
        let config = read_volatile(&self.registers.configuration) | 0b01;
        println!("general_config set: {:#b}", config);
        write_volatile(&mut self.registers.configuration, config);
        println!(
            "general_config set: {:#b}",
            read_volatile(&self.registers.configuration)
        );
    }
    /// # Safety
    /// This is safe only when HPET is globally disabled.
    unsafe fn setup(&mut self, index: usize, comparator_value: u64) {
        write_volatile(&mut self.registers.main_counter_value, 0);
        println!("general_caps: {:#018X}", self.registers.capabilities_and_id);
        let timer = &mut self.registers.timers[index];
        let mut config = read_volatile(&timer.configuration_and_capability);
        config &= !(TIMER_CONFIG_INT_ENABLE
            | TIMER_CONFIG_USE_PERIODIC_MODE
            | TIMER_CONFIG_LEVEL_TRIGGER
            | (0b11111 << 9));
        timer.write_config(config);
        let mut config = read_volatile(&timer.configuration_and_capability);
        config |= TIMER_CONFIG_INT_ENABLE | TIMER_CONFIG_USE_PERIODIC_MODE;
        timer.write_config(config);
        let mut config = read_volatile(&timer.configuration_and_capability);
        config |= TIMER_CONFIG_SET_COMPARATOR_VALUE;
        timer.write_config(config);
        write_volatile(&mut self.registers.main_counter_value, 0);
        write_volatile(&mut timer.comparator_value, comparator_value);
        println!("comparator_value: {}", comparator_value);
    }
    unsafe fn init(&mut self) {
        // c.f. 2.3.9.2.2 Periodic Mode
        // Ensure that legacy replacement routing (LEG_ROUTE_CAP) is supported.
        // assert!(self.registers.capabilities_and_id & (1 << 15) != 0);
        self.globally_disable();
        println!("{:?}", self);
        for i in 0..self.num_of_timers {
            println!("{:?}", self.registers.timers[i]);
        }
        self.setup(0, self.freq / 10);
        println!("{:?}", self);
        for i in 0..self.num_of_timers {
            println!("{:?}", self.registers.timers[i]);
        }
        self.globally_enable();
    }
    pub fn main_counter(&self) -> u64 {
        // This is safe as far as self is properly constructed.
        unsafe { read_volatile(&self.registers.main_counter_value) }
    }
    pub fn notify_end_of_interrupt(&mut self) {
        self.registers.interrupt_status.store(0, Ordering::Relaxed);
    }
}
impl fmt::Debug for Hpet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "HPET{{ int_status: {:#b}, main_counter: {}, timer0_comparator: {} }}",
            self.registers.interrupt_status.load(Ordering::Relaxed),
            self.main_counter(),
            self.registers.timers[0].comparator_value,
        )
    }
}
