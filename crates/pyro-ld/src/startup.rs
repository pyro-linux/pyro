use crate::linker::Linker;
use crate::syscall;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AuxvEntry {
	pub a_type: usize,
	pub a_val: usize,
}

pub const AT_NULL: usize = 0;
pub const AT_PHDR: usize = 3;
pub const AT_PHENT: usize = 4;
pub const AT_PHNUM: usize = 5;
pub const AT_ENTRY: usize = 9;
pub const AT_BASE: usize = 7;

pub fn entry_point() -> ! {
	let stack_ptr: *const usize;
	unsafe {
		core::arch::asm!(
			"mov {}, rsp",
			out(reg) stack_ptr,
			options(nostack)
		);
	}

	let mut linker = Linker::new();

	match linker.bootstrap(stack_ptr) {
		Ok(entry) => unsafe {
			let entry_fn: extern "C" fn() -> ! = core::mem::transmute(entry);
			entry_fn();
		},
		Err(_) => {
			let msg = b"Failed to bootstrap linker\n";
			syscall::write(2, msg);
			syscall::exit(1);
		}
	}
}

pub fn parse_auxv(mut auxv_ptr: *const AuxvEntry) -> ParsedAuxv {
	let mut result = ParsedAuxv::default();

	unsafe {
		loop {
			let entry = *auxv_ptr;
			match entry.a_type {
				AT_NULL => break,
				AT_PHDR => result.phdr = entry.a_val,
				AT_PHENT => result.phent = entry.a_val,
				AT_PHNUM => result.phnum = entry.a_val,
				AT_ENTRY => result.entry = entry.a_val,
				AT_BASE => result.base = entry.a_val,
				_ => {}
			}
			auxv_ptr = auxv_ptr.add(1);
		}
	}

	result
}

#[derive(Default)]
pub struct ParsedAuxv {
	pub phdr: usize,
	pub phent: usize,
	pub phnum: usize,
	pub entry: usize,
	pub base: usize,
}
