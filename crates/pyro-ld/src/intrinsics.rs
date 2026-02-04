#[no_mangle]
pub unsafe extern "C" fn memcpy(
	dest: *mut u8,
	src: *const u8,
	n: usize,
) -> *mut u8 {
	let mut i = 0;
	while i < n {
		*dest.add(i) = *src.add(i);
		i += 1;
	}
	dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
	let mut i = 0;
	while i < n {
		*s.add(i) = c as u8;
		i += 1;
	}
	s
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
	let mut i = 0;
	while i < n {
		let a = *s1.add(i);
		let b = *s2.add(i);
		if a != b {
			return a as i32 - b as i32;
		}
		i += 1;
	}
	0
}

#[no_mangle]
pub unsafe extern "C" fn memmove(
	dest: *mut u8,
	src: *const u8,
	n: usize,
) -> *mut u8 {
	if src < dest as *const u8 {
		let mut i = n;
		while i > 0 {
			i -= 1;
			*dest.add(i) = *src.add(i);
		}
	} else {
		let mut i = 0;
		while i < n {
			*dest.add(i) = *src.add(i);
			i += 1;
		}
	}
	dest
}

#[no_mangle]
pub unsafe extern "C" fn bcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
	memcmp(s1, s2, n)
}

// Unwinding stubs for debug builds
// Since we use panic=abort, these should never be called
#[no_mangle]
pub extern "C" fn _Unwind_Resume() {
	unsafe {
		core::arch::asm!(
			"mov rdi, 1",
			"mov rax, 60",
			"syscall",
			options(noreturn)
		);
	}
}

#[no_mangle]
pub extern "C" fn rust_eh_personality() {}

#[no_mangle]
pub extern "C" fn rust_begin_unwind(_info: &core::panic::PanicInfo) -> ! {
	unsafe {
		core::arch::asm!(
			"mov rdi, 1",
			"mov rax, 60",
			"syscall",
			options(noreturn)
		);
	}
}
