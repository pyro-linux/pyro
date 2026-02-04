// Simple test to verify the allocator works
// This will be used when we can actually run the linker

#[cfg(test)]
mod tests {
	extern crate alloc;
	use alloc::string::String;
	use alloc::vec::Vec;

	#[test]
	fn test_allocator_basic() {
		// Test Vec allocation
		let mut v = Vec::new();
		v.push(1);
		v.push(2);
		v.push(3);
		assert_eq!(v.len(), 3);
		assert_eq!(v[0], 1);
		assert_eq!(v[1], 2);
		assert_eq!(v[2], 3);
	}

	#[test]
	fn test_allocator_string() {
		// Test String allocation
		let mut s = String::new();
		s.push_str("Hello");
		s.push_str(" ");
		s.push_str("World");
		assert_eq!(s, "Hello World");
	}

	#[test]
	fn test_allocator_large() {
		// Test larger allocations
		let v: Vec<u8> = vec![0; 100_000];
		assert_eq!(v.len(), 100_000);
	}

	#[test]
	fn test_allocator_many_small() {
		// Test many small allocations
		let mut vecs = Vec::new();
		for i in 0..1000 {
			let mut v = Vec::new();
			v.push(i);
			vecs.push(v);
		}
		assert_eq!(vecs.len(), 1000);
	}
}
