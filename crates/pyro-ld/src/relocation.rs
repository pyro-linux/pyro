use crate::elf::DynamicInfo;
use crate::linker::Linker;

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Rela {
	pub r_offset: u64,
	pub r_info: u64,
	pub r_addend: i64,
}

impl Rela {
	pub fn r_type(&self) -> u32 {
		(self.r_info & 0xffffffff) as u32
	}

	pub fn r_sym(&self) -> u32 {
		(self.r_info >> 32) as u32
	}
}

pub struct RelocationEngine<'a> {
	base_addr: usize,
	linker: &'a Linker,
}

impl<'a> RelocationEngine<'a> {
	pub fn new(base_addr: usize, linker: &'a Linker) -> Self {
		RelocationEngine { base_addr, linker }
	}

	pub fn apply_relocations(
		&self,
		dyn_info: &DynamicInfo,
	) -> Result<(), &'static str> {
		if dyn_info.rela != 0 && dyn_info.relasz > 0 {
			let rela_count = dyn_info.relasz / dyn_info.relaent;
			let rela_slice = unsafe {
				core::slice::from_raw_parts(
					dyn_info.rela as *const Rela,
					rela_count,
				)
			};

			for rela in rela_slice {
				self.apply_rela(rela, dyn_info)?;
			}
		}

		if dyn_info.jmprel != 0 && dyn_info.pltrelsz > 0 {
			let plt_count = dyn_info.pltrelsz / core::mem::size_of::<Rela>();
			let plt_slice = unsafe {
				core::slice::from_raw_parts(
					dyn_info.jmprel as *const Rela,
					plt_count,
				)
			};

			for rela in plt_slice {
				self.apply_rela(rela, dyn_info)?;
			}
		}

		Ok(())
	}

	fn apply_rela(
		&self,
		rela: &Rela,
		dyn_info: &DynamicInfo,
	) -> Result<(), &'static str> {
		let r_type = rela.r_type();
		let r_offset = rela.r_offset as usize;
		let r_addend = rela.r_addend;
		let r_sym = rela.r_sym();

		let reloc_addr = self.base_addr + r_offset;

		match r_type {
			R_X86_64_NONE => {}

			R_X86_64_RELATIVE => {
				let value = self.base_addr.wrapping_add(r_addend as usize);
				unsafe {
					*(reloc_addr as *mut usize) = value;
				}
			}

			R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
				// These require symbol resolution
				if r_sym != 0 {
					if let Some(sym) = dyn_info.get_symbol(r_sym as usize) {
						let sym_name =
							dyn_info.get_string(sym.st_name as usize);

						// Try to resolve symbol
						let sym_addr = if sym.st_value != 0 {
							// Symbol defined in this object
							self.base_addr + sym.st_value as usize
						} else if let Some(addr) =
							self.linker.resolve_symbol(sym_name)
						{
							// Symbol found in another object
							addr
						} else {
							// Symbol not found - might be weak or optional
							0
						};

						let value = sym_addr.wrapping_add(r_addend as usize);
						unsafe {
							*(reloc_addr as *mut usize) = value;
						}
					}
				} else {
					let value = self.base_addr.wrapping_add(r_addend as usize);
					unsafe {
						*(reloc_addr as *mut usize) = value;
					}
				}
			}

			R_X86_64_64 => {
				// Absolute relocation
				let value = if r_sym != 0 {
					if let Some(sym) = dyn_info.get_symbol(r_sym as usize) {
						let sym_name =
							dyn_info.get_string(sym.st_name as usize);
						let sym_addr = if sym.st_value != 0 {
							self.base_addr + sym.st_value as usize
						} else if let Some(addr) =
							self.linker.resolve_symbol(sym_name)
						{
							addr
						} else {
							0
						};
						sym_addr.wrapping_add(r_addend as usize)
					} else {
						self.base_addr.wrapping_add(r_addend as usize)
					}
				} else {
					self.base_addr.wrapping_add(r_addend as usize)
				};

				unsafe {
					*(reloc_addr as *mut usize) = value;
				}
			}

			_ => return Err("Unsupported relocation type"),
		}

		Ok(())
	}
}
