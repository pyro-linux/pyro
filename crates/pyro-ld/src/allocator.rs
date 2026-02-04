use crate::syscall::{
	mmap, MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, PROT_READ, PROT_WRITE,
};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
const MIN_ALIGN: usize = 16;

struct BumpAllocator {
	next: AtomicUsize,
	end: AtomicUsize,
}

impl BumpAllocator {
	const fn new() -> Self {
		BumpAllocator {
			next: AtomicUsize::new(0),
			end: AtomicUsize::new(0),
		}
	}

	unsafe fn allocate_chunk(&self, size: usize) -> *mut u8 {
		let chunk_size = size.max(CHUNK_SIZE);

		let ptr = mmap(
			null_mut(),
			chunk_size,
			PROT_READ | PROT_WRITE,
			MAP_PRIVATE | MAP_ANONYMOUS,
			-1,
			0,
		);

		if ptr == MAP_FAILED {
			return null_mut();
		}

		self.next.store(ptr as usize, Ordering::Release);
		self.end.store(ptr as usize + chunk_size, Ordering::Release);

		ptr
	}

	unsafe fn alloc_from_chunk(&self, layout: Layout) -> *mut u8 {
		let size = layout.size();
		let align = layout.align().max(MIN_ALIGN);

		loop {
			let next = self.next.load(Ordering::Acquire);
			let end = self.end.load(Ordering::Acquire);

			if next == 0 || end == 0 {
				if self.allocate_chunk(size) == null_mut() {
					return null_mut();
				}
				continue;
			}

			let aligned = (next + align - 1) & !(align - 1);
			let new_next = aligned + size;

			if new_next > end {
				if self.allocate_chunk(size) == null_mut() {
					return null_mut();
				}
				continue;
			}

			if self
				.next
				.compare_exchange(
					next,
					new_next,
					Ordering::Release,
					Ordering::Acquire,
				)
				.is_ok()
			{
				return aligned as *mut u8;
			}
		}
	}
}

unsafe impl GlobalAlloc for BumpAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		self.alloc_from_chunk(layout)
	}

	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
		// Bump allocator doesn't free individual allocations
		// Memory is released when the process exits
	}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();
