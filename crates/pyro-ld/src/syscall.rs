pub fn exit(code: i32) -> ! {
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 60_u64, // SYS_exit
			in("rdi") code as u64,
			options(noreturn)
		);
	}
}

pub fn write(fd: i32, buf: &[u8]) -> isize {
	let result: isize;
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 1_u64, // SYS_write
			in("rdi") fd as u64,
			in("rsi") buf.as_ptr() as u64,
			in("rdx") buf.len() as u64,
			lateout("rax") result,
			options(nostack)
		);
	}
	result
}

// File operations
pub const O_RDONLY: i32 = 0;
pub const O_CLOEXEC: i32 = 0x80000;

pub fn open(path: *const u8, flags: i32) -> i32 {
	let result: isize;
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 2_u64, // SYS_open
			in("rdi") path as u64,
			in("rsi") flags as u64,
			lateout("rax") result,
			options(nostack)
		);
	}
	result as i32
}

pub fn close(fd: i32) -> i32 {
	let result: isize;
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 3_u64, // SYS_close
			in("rdi") fd as u64,
			lateout("rax") result,
			options(nostack)
		);
	}
	result as i32
}

pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
	let result: isize;
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 0_u64, // SYS_read
			in("rdi") fd as u64,
			in("rsi") buf as u64,
			in("rdx") count as u64,
			lateout("rax") result,
			options(nostack)
		);
	}
	result
}

pub fn lseek(fd: i32, offset: i64, whence: i32) -> i64 {
	let result: isize;
	unsafe {
		core::arch::asm!(
			"syscall",
			in("rax") 8_u64, // SYS_lseek
			in("rdi") fd as u64,
			in("rsi") offset as u64,
			in("rdx") whence as u64,
			lateout("rax") result,
			options(nostack)
		);
	}
	result as i64
}

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

// Memory protection flags
pub const PROT_READ: i32 = 0x1;
pub const PROT_WRITE: i32 = 0x2;
#[allow(dead_code)]
pub const PROT_EXEC: i32 = 0x4;

// Memory mapping flags
pub const MAP_PRIVATE: i32 = 0x02;
pub const MAP_ANONYMOUS: i32 = 0x20;
pub const MAP_FAILED: *mut u8 = !0 as *mut u8;

pub unsafe fn mmap(
	addr: *mut u8,
	length: usize,
	prot: i32,
	flags: i32,
	fd: i32,
	offset: i64,
) -> *mut u8 {
	let result: isize;
	core::arch::asm!(
		"syscall",
		in("rax") 9_u64, // SYS_mmap
		in("rdi") addr as u64,
		in("rsi") length as u64,
		in("rdx") prot as u64,
		in("r10") flags as u64,
		in("r8") fd as u64,
		in("r9") offset as u64,
		lateout("rax") result,
		options(nostack)
	);

	if result < 0 && result >= -4095 {
		MAP_FAILED
	} else {
		result as *mut u8
	}
}

#[allow(dead_code)]
pub unsafe fn munmap(addr: *mut u8, length: usize) -> i32 {
	let result: isize;
	core::arch::asm!(
		"syscall",
		in("rax") 11_u64, // SYS_munmap
		in("rdi") addr as u64,
		in("rsi") length as u64,
		lateout("rax") result,
		options(nostack)
	);
	result as i32
}
