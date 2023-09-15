use crate::error::Result;
use crate::executor::yield_execution;
use crate::executor::TimeoutFuture;
use crate::println;
use crate::x86_64::read_io_port_u8;

pub async fn keyboard_task() -> Result<()> {
    loop {
        const PORT_PS2_KBD_CMD_AND_STATUS: u16 = 0x64;
        const PORT_PS2_KBD_DATA: u16 = 0x60;
        const BIT_PS2_KBD_CMD_AND_STATUS_DATA_READY: u8 = 0x01;
        let kbd_status = read_io_port_u8(PORT_PS2_KBD_CMD_AND_STATUS);
        if kbd_status & BIT_PS2_KBD_CMD_AND_STATUS_DATA_READY != 0 {
            let kbd_data = read_io_port_u8(PORT_PS2_KBD_DATA);
            println!("KBD: {kbd_data}");
        }
        TimeoutFuture::new_ms(10).await;
        yield_execution().await;
    }
}
