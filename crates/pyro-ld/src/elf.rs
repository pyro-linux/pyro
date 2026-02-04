use goblin::elf::dynamic::{
	Dyn, DT_GNU_HASH, DT_HASH, DT_JMPREL, DT_NEEDED, DT_PLTREL, DT_PLTRELSZ,
	DT_REL, DT_RELA, DT_RELAENT, DT_RELASZ, DT_RELENT, DT_RELSZ, DT_STRTAB,
	DT_SYMENT, DT_SYMTAB,
};
use goblin::elf::header::Header;
use goblin::elf::program_header::{ProgramHeader, PT_DYNAMIC};
use goblin::elf::sym::Sym;
use scroll::Pread;

pub struct ElfFile {
	pub base_addr: usize,
	pub header: Header,
	pub load_bias: usize,
}

impl ElfFile {
	pub fn new(base_addr: usize) -> Result<Self, &'static str> {
		let header_bytes =
			unsafe { core::slice::from_raw_parts(base_addr as *const u8, 64) };

		let header = header_bytes
			.pread::<Header>(0)
			.map_err(|_| "Failed to parse ELF header")?;

		Ok(ElfFile {
			base_addr,
			header,
			load_bias: 0,
		})
	}

	pub fn program_headers(&self) -> &[ProgramHeader] {
		let phdr_addr = self.base_addr + self.header.e_phoff as usize;
		unsafe {
			core::slice::from_raw_parts(
				phdr_addr as *const ProgramHeader,
				self.header.e_phnum as usize,
			)
		}
	}

	pub fn find_dynamic(&self) -> Option<&[Dyn]> {
		for phdr in self.program_headers() {
			if phdr.p_type == PT_DYNAMIC {
				let dyn_addr = self.base_addr + phdr.p_vaddr as usize;
				let dyn_count = (phdr.p_memsz
					/ core::mem::size_of::<Dyn>() as u64)
					as usize;
				let dyn_slice = unsafe {
					core::slice::from_raw_parts(
						dyn_addr as *const Dyn,
						dyn_count,
					)
				};
				return Some(dyn_slice);
			}
		}
		None
	}
}

pub struct DynamicInfo {
	pub strtab: usize,
	pub symtab: usize,
	pub syment: usize,
	pub hash: usize,
	pub gnu_hash: usize,
	pub rela: usize,
	pub relasz: usize,
	pub relaent: usize,
	pub rel: usize,
	pub relsz: usize,
	pub relent: usize,
	pub jmprel: usize,
	pub pltrelsz: usize,
	pub pltrel: u64,
	pub needed: [usize; 16],
	pub needed_count: usize,
}

impl DynamicInfo {
	pub fn parse(dynamic: &[Dyn], base_addr: usize) -> Self {
		let mut info = DynamicInfo {
			strtab: 0,
			symtab: 0,
			syment: core::mem::size_of::<Sym>(),
			hash: 0,
			gnu_hash: 0,
			rela: 0,
			relasz: 0,
			relaent: 0,
			rel: 0,
			relsz: 0,
			relent: 0,
			jmprel: 0,
			pltrelsz: 0,
			pltrel: 0,
			needed: [0; 16],
			needed_count: 0,
		};

		for dyn_entry in dynamic {
			match dyn_entry.d_tag {
				DT_STRTAB => info.strtab = base_addr + dyn_entry.d_val as usize,
				DT_SYMTAB => info.symtab = base_addr + dyn_entry.d_val as usize,
				DT_SYMENT => info.syment = dyn_entry.d_val as usize,
				DT_HASH => info.hash = base_addr + dyn_entry.d_val as usize,
				DT_GNU_HASH => {
					info.gnu_hash = base_addr + dyn_entry.d_val as usize
				}
				DT_RELA => info.rela = base_addr + dyn_entry.d_val as usize,
				DT_RELASZ => info.relasz = dyn_entry.d_val as usize,
				DT_RELAENT => info.relaent = dyn_entry.d_val as usize,
				DT_REL => info.rel = base_addr + dyn_entry.d_val as usize,
				DT_RELSZ => info.relsz = dyn_entry.d_val as usize,
				DT_RELENT => info.relent = dyn_entry.d_val as usize,
				DT_JMPREL => info.jmprel = base_addr + dyn_entry.d_val as usize,
				DT_PLTRELSZ => info.pltrelsz = dyn_entry.d_val as usize,
				DT_PLTREL => info.pltrel = dyn_entry.d_val,
				DT_NEEDED => {
					if info.needed_count < info.needed.len() {
						info.needed[info.needed_count] =
							dyn_entry.d_val as usize;
						info.needed_count += 1;
					}
				}
				_ => {}
			}
		}

		info
	}

	pub fn get_string(&self, offset: usize) -> &str {
		if self.strtab == 0 {
			return "";
		}

		unsafe {
			let ptr = (self.strtab + offset) as *const u8;
			let mut len = 0;
			while *ptr.add(len) != 0 {
				len += 1;
				if len > 4096 {
					return "";
				}
			}
			let bytes = core::slice::from_raw_parts(ptr, len);
			core::str::from_utf8_unchecked(bytes)
		}
	}

	pub fn get_symbol(&self, index: usize) -> Option<&Sym> {
		if self.symtab == 0 || index == 0 {
			return None;
		}

		unsafe {
			let sym_ptr = (self.symtab + index * self.syment) as *const Sym;
			Some(&*sym_ptr)
		}
	}
}
