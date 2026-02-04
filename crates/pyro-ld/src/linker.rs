extern crate alloc;
use crate::elf::{DynamicInfo, ElfFile};
use crate::loader::FileLoader;
use crate::relocation::RelocationEngine;
use crate::startup::{parse_auxv, AuxvEntry};
use crate::symbol::SymbolResolver;
use alloc::string::String;
use alloc::vec::Vec;

pub struct Linker {
	loaded_objects: [Option<LoadedObject>; 16],
	loaded_count: usize,
	search_paths: Vec<String>,
}

pub struct LoadedObject {
	pub base_addr: usize,
	pub elf: ElfFile,
	pub dyn_info: Option<DynamicInfo>,
	pub name: String,
}

impl Linker {
	pub fn new() -> Self {
		let mut search_paths = Vec::new();
		search_paths.push(String::from("/lib"));
		search_paths.push(String::from("/usr/lib"));
		search_paths.push(String::from("/lib64"));
		search_paths.push(String::from("/usr/lib64"));

		const INIT: Option<LoadedObject> = None;
		Linker {
			loaded_objects: [INIT; 16],
			loaded_count: 0,
			search_paths,
		}
	}

	pub fn bootstrap(
		&mut self,
		stack_ptr: *const usize,
	) -> Result<usize, &'static str> {
		let argc = unsafe { *stack_ptr };
		let argv_ptr = unsafe { stack_ptr.add(1) };
		let envp_ptr = unsafe { argv_ptr.add(argc + 1) };

		let mut auxv_ptr = envp_ptr;
		unsafe {
			while *auxv_ptr != 0 {
				auxv_ptr = auxv_ptr.add(1);
			}
			auxv_ptr = auxv_ptr.add(1);
		}

		let auxv = parse_auxv(auxv_ptr as *const AuxvEntry);

		if auxv.base != 0 {
			let elf = ElfFile::new(auxv.base)?;

			if let Some(dynamic) = elf.find_dynamic() {
				let dyn_info = DynamicInfo::parse(dynamic, auxv.base);

				// Load dependencies first
				for i in 0..dyn_info.needed_count {
					let lib_name = dyn_info.get_string(dyn_info.needed[i]);
					if !lib_name.is_empty() {
						let _ = self.load_library(lib_name);
					}
				}

				let reloc_engine = RelocationEngine::new(auxv.base, self);
				reloc_engine.apply_relocations(&dyn_info)?;

				self.loaded_objects[0] = Some(LoadedObject {
					base_addr: auxv.base,
					elf,
					dyn_info: Some(dyn_info),
					name: String::from("<main>"),
				});
				self.loaded_count = 1;
			}
		}

		if auxv.entry != 0 {
			Ok(auxv.entry)
		} else {
			Err("No entry point found")
		}
	}

	pub fn load_library(&mut self, name: &str) -> Result<usize, &'static str> {
		// Check if already loaded
		for obj in &self.loaded_objects[..self.loaded_count] {
			if let Some(obj) = obj {
				if obj.name == name {
					return Ok(obj.base_addr);
				}
			}
		}

		if self.loaded_count >= self.loaded_objects.len() {
			return Err("Too many loaded objects");
		}

		// Try to find library in search paths
		let mut found_path = None;
		for path in &self.search_paths {
			let full_path = String::from(path.as_str()) + "/" + name;
			// Try to open - if successful, we found it
			if FileLoader::load_file(&full_path).is_ok() {
				found_path = Some(full_path);
				break;
			}
		}

		let lib_path = found_path.ok_or("Library not found in search paths")?;

		// Load the ELF file
		let base_addr = FileLoader::load_elf(&lib_path)?;
		let elf = ElfFile::new(base_addr)?;

		let dyn_info = if let Some(dynamic) = elf.find_dynamic() {
			let info = DynamicInfo::parse(dynamic, base_addr);

			// Recursively load dependencies
			for i in 0..info.needed_count {
				let dep_name = info.get_string(info.needed[i]);
				if !dep_name.is_empty() {
					let _ = self.load_library(dep_name);
				}
			}

			// Apply relocations
			let reloc_engine = RelocationEngine::new(base_addr, self);
			reloc_engine.apply_relocations(&info)?;

			Some(info)
		} else {
			None
		};

		let idx = self.loaded_count;
		self.loaded_objects[idx] = Some(LoadedObject {
			base_addr,
			elf,
			dyn_info,
			name: String::from(name),
		});
		self.loaded_count += 1;

		Ok(base_addr)
	}

	pub fn resolve_symbol(&self, name: &str) -> Option<usize> {
		// Build list of (base_addr, dyn_info) for lookup
		let mut objects = Vec::new();
		for obj in &self.loaded_objects[..self.loaded_count] {
			if let Some(obj) = obj {
				if let Some(ref dyn_info) = obj.dyn_info {
					objects.push((obj.base_addr, dyn_info));
				}
			}
		}

		SymbolResolver::lookup_in_objects(name, &objects)
	}
}
