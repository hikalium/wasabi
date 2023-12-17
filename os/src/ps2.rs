use crate::error::Error::Failed;
use crate::error::Result;
use crate::executor::yield_execution;
use crate::executor::TimeoutFuture;
use crate::input::InputManager;
use crate::x86_64::read_io_port_u8;

const PORT_PS2_KBD_CMD_AND_STATUS: u16 = 0x64;
const PORT_PS2_KBD_DATA: u16 = 0x60;

async fn kbc_block_until_recv_ready() -> Result<()> {
    const BIT_PS2_KBD_CMD_AND_STATUS_RECV_READY: u8 = 0b0001;
    for _ in 0..100 {
        let kbd_status = read_io_port_u8(PORT_PS2_KBD_CMD_AND_STATUS);
        if kbd_status & BIT_PS2_KBD_CMD_AND_STATUS_RECV_READY != 0 {
            return Ok(());
        }
        TimeoutFuture::new_ms(10).await;
        yield_execution().await;
    }
    Err(Failed("kbc_block_until_recv_ready: timed out"))
}

pub async fn keyboard_task() -> Result<()> {
    loop {
        if kbc_block_until_recv_ready().await.is_err() {
            yield_execution().await;
        }
        let kbd_data = read_io_port_u8(PORT_PS2_KBD_DATA);
        InputManager::take().push_input(kbd_data as char);
    }
}
