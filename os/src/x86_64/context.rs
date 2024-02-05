use crate::error::Result;
use crate::executor::yield_execution;
use crate::x86_64::ExecutionContext;
use crate::x86_64::CONTEXT_APP;
use crate::x86_64::CONTEXT_OS;
use core::arch::asm;
use core::mem::size_of;

pub async fn exec_app_context() -> Result<i64> {
    let mut retcode: i64;
    loop {
        let mut exit_reason: i64;
        unsafe {
            let os_ctx = CONTEXT_OS.lock().as_mut_ptr();
            let (app_rsp, app_rip, app_ctx_ptr) = {
                // Release the lock of CONTEXT_APP before entering the app to make it available
                // from syscall handlers.
                let mut app_ctx = CONTEXT_APP.lock();
                (app_ctx.cpu.rsp, app_ctx.cpu.rip, app_ctx.as_mut_ptr())
            };
            asm!(
                // Save current execution state into CONTEXT_OS(rsi)
                // General registers
                "xchg rsp,rsi", // swap rsi with rsp to utilize push/pop
                "push rsi", // ExecutionContext.rsp
                "push r15",
                "push r14",
                "push r13",
                "push r12",
                "push r11",
                "push r10",
                "push r9",
                "push r8",
                "push rdi",
                "push rsi",
                "push rbp",
                "push rbx",
                "push rdx",
                "push rcx",
                "push rax",
                "pushfq", // ExecutionContext.rflags
                "lea r8, [rip+0f]", // ExecutionContext.rip, return address from app
                "push r8", // ExecutionContext.rip
                "sub rsp, 512",
                "fxsave64[rsp]",
                "xchg rsp,rsi", // recover the original rsp
                // At this point, the current CPU state is saved to CONTEXT_OS

                // Set data segments to USER_DS
                // rdx is passed from the Rust code (see the last part of this asm block).
                "mov es, dx",
                "mov ds, dx",
                "mov fs, dx",
                "mov gs, dx",

                // Prepare the stack to use iretq to switch to user mode
                "mov rsp, rax", // RSP = stack for app
                // Push values needed by `return_to_app`
                "push rdi", // RIP to resume
                "push 2", // RFLAGS to resume
                //
                "push rbx",
                "push rbp",
                "push r15",
                "push r14",
                "push r13",
                "push r12",
                //
                "push r10",
                "push r9",
                "push r8",
                "push rdi",
                "push rsi",
                "push rdx",
                "push rax",
                // Start (or resume) the app execution
                ".global return_to_app", // external symbol
                "jmp return_to_app",

                // ---- no one will pass through here ----

                // **** return from app via return_to_os() ****
                // See also: os/src/syscall.rs
                "0:",
                // At this point:
                // - context: CONTEXT_APP + handling syscall
                //   - so it's in the kernel mode
                // - rdi: addr of CONTEXT_OS
                // - rsi: addr of CONTEXT_APP

                // Recover the segment registers to OS
                "push rdi", // Use rdi as TMP
                "mov di,ss", // SS is already pointing the OS data segment
                             // (which is done by the CPU when entering syscall)
                             // so copy it to other segment registers.
                "mov ds,di",
                "mov es,di",
                "mov fs,di",
                "mov gs,di",
                "pop rdi",  // Recover rdi value

                // Save the cpu state to CONTEXT_APP

                // Load the cpu state from CONTEXT_OS
                "xchg rsp, rdi", // swap rsp and rdi to utilize push / pop
                "fxrstor64[rsp]",
                "add rsp, 512",
                "pop rax", // Skip RIP
                "popfq", // Restore RFLAGS
                "pop rax",
                "pop rcx",
                "pop rdx",
                "pop rbx",
                "pop rbp",
                "pop rsi",
                "pop rdi",
                "pop r8",
                "pop r9",
                "pop r10",
                "pop r11",
                "pop r12",
                "pop r13",
                "pop r14",
                "pop r15",
                "pop rsp",
                // At this point, the CPU state is same as the CONTEXT_OS except for RIP.
                // Returning to the Rust code and continue the execution.

                in("rax") app_rsp,
                in("rcx") crate::x86_64::USER64_CS,
                in("rdx") crate::x86_64::USER_DS,
                // rbx is used for LLVM internally
                in("rsi") (os_ctx as *mut u8).add(size_of::<ExecutionContext>()),
                in("r9") (app_ctx_ptr as *mut u8).add(size_of::<ExecutionContext>()),
                in("rdi") app_rip,
                lateout("rax") retcode,
                lateout("r8") exit_reason,
            );
        }
        if exit_reason == 0 {
            // return to os
            break;
        }
        yield_execution().await;
    }
    Ok(retcode)
}
