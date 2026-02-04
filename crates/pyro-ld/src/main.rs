#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod elf;
mod intrinsics;
mod linker;
mod loader;
mod relocation;
mod startup;
mod symbol;
mod syscall;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	syscall::exit(1)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
	startup::entry_point()
}
