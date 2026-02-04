extern crate alloc;
use crate::syscall::{
	close, lseek, mmap, open, read, MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE,
	O_CLOEXEC, O_RDONLY, PROT_READ, PROT_WRITE, SEEK_END, SEEK_SET,
};
use alloc::vec::Vec;
use goblin::elf::header::Header;
use goblin::elf::program_header::{ProgramHeader, PT_LOAD};
use scroll::Pread;

pub struct FileLoader;

impl FileLoader {
	pub fn load_file(path: &str) -> Result<Vec<u8>, &'static str> {
		// Convert path to null-terminated string
		let mut path_bytes = Vec::new();
		path_bytes.extend_from_slice(path.as_bytes());
		path_bytes.push(0);

		let fd = unsafe { open(path_bytes.as_ptr(), O_RDONLY | O_CLOEXEC) };
		if fd < 0 {
			return Err("Failed to open file");
		}

		// Get file size
		let size = unsafe { lseek(fd, 0, SEEK_END) };
		if size < 0 {
			unsafe { close(fd) };
			return Err("Failed to get file size");
		}

		unsafe { lseek(fd, 0, SEEK_SET) };

		// Read file contents
		let mut buffer: Vec<u8> = Vec::with_capacity(size as usize);
		unsafe {
			buffer.set_len(size as usize);
			let mut total_read = 0;
			while total_read < size as usize {
				let n = read(
					fd,
					buffer.as_mut_ptr().add(total_read),
					size as usize - total_read,
				);
				if n <= 0 {
					close(fd);
					return Err("Failed to read file");
				}
				total_read += n as usize;
			}
			close(fd);
		}

		Ok(buffer)
	}

	pub fn load_elf(path: &str) -> Result<usize, &'static str> {
		let file_data = Self::load_file(path)?;

		// Parse ELF header
		let header: Header = file_data
			.pread(0)
			.map_err(|_| "Failed to parse ELF header")?;

		if header.e_type != goblin::elf::header::ET_DYN
			&& header.e_type != goblin::elf::header::ET_EXEC
		{
			return Err("Not a valid executable or shared library");
		}

		// Find PT_LOAD segments and calculate total size
		let phdrs_offset = header.e_phoff as usize;
		let phdr_size = header.e_phentsize as usize;
		let phdr_count = header.e_phnum as usize;

		let mut min_vaddr = usize::MAX;
		let mut max_vaddr = 0usize;

		for i in 0..phdr_count {
			let offset = phdrs_offset + i * phdr_size;
			let phdr: ProgramHeader = file_data
				.pread(offset)
				.map_err(|_| "Failed to parse program header")?;

			if phdr.p_type == PT_LOAD {
				let start = phdr.p_vaddr as usize;
				let end = start + phdr.p_memsz as usize;
				min_vaddr = min_vaddr.min(start);
				max_vaddr = max_vaddr.max(end);
			}
		}

		if min_vaddr == usize::MAX {
			return Err("No loadable segments found");
		}

		// Calculate total size needed
		let total_size = max_vaddr - min_vaddr;
		let page_size = 4096;
		let aligned_size = (total_size + page_size - 1) & !(page_size - 1);

		// Allocate memory for the entire image
		let base = unsafe {
			mmap(
				core::ptr::null_mut(),
				aligned_size,
				PROT_READ | PROT_WRITE,
				MAP_PRIVATE | MAP_ANONYMOUS,
				-1,
				0,
			)
		};

		if base == MAP_FAILED {
			return Err("Failed to allocate memory for ELF");
		}

		let base_addr = base as usize;
		let load_bias = base_addr.wrapping_sub(min_vaddr);

		// Load all PT_LOAD segments
		for i in 0..phdr_count {
			let offset = phdrs_offset + i * phdr_size;
			let phdr: ProgramHeader = file_data
				.pread(offset)
				.map_err(|_| "Failed to parse program header")?;

			if phdr.p_type == PT_LOAD {
				let dest_addr = load_bias + phdr.p_vaddr as usize;
				let file_offset = phdr.p_offset as usize;
				let file_size = phdr.p_filesz as usize;
				let mem_size = phdr.p_memsz as usize;

				// Copy file data
				if file_size > 0 {
					unsafe {
						core::ptr::copy_nonoverlapping(
							file_data.as_ptr().add(file_offset),
							dest_addr as *mut u8,
							file_size,
						);
					}
				}

				// Zero the BSS section
				if mem_size > file_size {
					unsafe {
						core::ptr::write_bytes(
							(dest_addr + file_size) as *mut u8,
							0,
							mem_size - file_size,
						);
					}
				}

				// TODO: Set proper permissions based on phdr.p_flags
				// For now we leave everything RW for simplicity
			}
		}

		Ok(base_addr)
	}
}
