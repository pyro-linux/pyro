use crate::elf::DynamicInfo;

pub struct SymbolResolver;

impl SymbolResolver {
	// ELF hash function
	fn elf_hash(name: &[u8]) -> u32 {
		let mut h: u32 = 0;
		for &b in name {
			h = (h << 4).wrapping_add(b as u32);
			let g = h & 0xf0000000;
			if g != 0 {
				h ^= g >> 24;
			}
			h &= !g;
		}
		h
	}

	// GNU hash function
	fn gnu_hash(name: &[u8]) -> u32 {
		let mut h: u32 = 5381;
		for &b in name {
			h = h.wrapping_mul(33).wrapping_add(b as u32);
		}
		h
	}

	pub fn lookup_symbol(
		name: &str,
		dyn_info: &DynamicInfo,
		base_addr: usize,
	) -> Option<usize> {
		// Try GNU hash first (faster)
		if dyn_info.gnu_hash != 0 {
			if let Some(addr) = Self::lookup_gnu_hash(name, dyn_info, base_addr)
			{
				return Some(addr);
			}
		}

		// Fallback to ELF hash
		if dyn_info.hash != 0 {
			return Self::lookup_elf_hash(name, dyn_info, base_addr);
		}

		None
	}

	fn lookup_elf_hash(
		name: &str,
		dyn_info: &DynamicInfo,
		base_addr: usize,
	) -> Option<usize> {
		if dyn_info.hash == 0 || dyn_info.symtab == 0 {
			return None;
		}

		unsafe {
			let hash_table = dyn_info.hash as *const u32;
			let nbucket = *hash_table;
			let nchain = *hash_table.add(1);
			let buckets = hash_table.add(2);
			let chains = buckets.add(nbucket as usize);

			let hash = Self::elf_hash(name.as_bytes());
			let mut symidx = *buckets.add((hash % nbucket) as usize);

			while symidx != 0 {
				if let Some(sym) = dyn_info.get_symbol(symidx as usize) {
					let sym_name = dyn_info.get_string(sym.st_name as usize);
					if sym_name == name {
						// Found the symbol
						if sym.st_value != 0 {
							return Some(base_addr + sym.st_value as usize);
						}
					}
				}

				if symidx >= nchain {
					break;
				}
				symidx = *chains.add(symidx as usize);
			}
		}

		None
	}

	fn lookup_gnu_hash(
		name: &str,
		dyn_info: &DynamicInfo,
		base_addr: usize,
	) -> Option<usize> {
		if dyn_info.gnu_hash == 0 || dyn_info.symtab == 0 {
			return None;
		}

		unsafe {
			let hash_table = dyn_info.gnu_hash as *const u32;
			let nbuckets = *hash_table;
			let symoffset = *hash_table.add(1);
			let bloom_size = *hash_table.add(2);
			let _bloom_shift = *hash_table.add(3);

			// For simplicity, skip bloom filter and search linearly
			// A full implementation would use the bloom filter for quick rejection

			let buckets = hash_table.add(4 + bloom_size as usize * 2);

			let hash = Self::gnu_hash(name.as_bytes());
			let bucket_idx = (hash % nbuckets) as usize;
			let mut symidx = *buckets.add(bucket_idx) as usize;

			if symidx < symoffset as usize {
				return None;
			}

			let chain_base = buckets.add(nbuckets as usize);

			loop {
				if let Some(sym) = dyn_info.get_symbol(symidx) {
					let sym_name = dyn_info.get_string(sym.st_name as usize);
					if sym_name == name {
						if sym.st_value != 0 {
							return Some(base_addr + sym.st_value as usize);
						}
					}
				}

				let chain_entry = *chain_base.add(symidx - symoffset as usize);
				if chain_entry & 1 != 0 {
					// End of chain
					break;
				}
				symidx += 1;
			}
		}

		None
	}

	// Lookup symbol across multiple loaded objects
	pub fn lookup_in_objects(
		name: &str,
		objects: &[(usize, &DynamicInfo)],
	) -> Option<usize> {
		for (base_addr, dyn_info) in objects {
			if let Some(addr) = Self::lookup_symbol(name, dyn_info, *base_addr)
			{
				return Some(addr);
			}
		}
		None
	}
}
