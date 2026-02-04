# Pyro - Rust-based Linux Dynamic Linker and Libc

A foundational implementation of a Linux dynamic linker (`ld.so`) written in Rust, serving as the first component of a glibc-compatible libc project.

## Project Structure

```
pyro/
├── crates/
│   └── pyro-ld/        # Dynamic linker implementation
├── Cargo.toml          # Workspace configuration
└── README.md          # This file
```

## Components

### pyro-ld - Dynamic Linker

A `no_std` dynamic linker that can:
- Bootstrap itself from the Linux kernel
- Parse ELF headers and segments
- Process relocations (R_X86_64_RELATIVE, R_X86_64_GLOB_DAT, R_X86_64_JUMP_SLOT)
- Transfer control to loaded programs

**Status:** ✅ Built successfully (24KB statically-linked executable)

See [crates/pyro-ld/README.md](crates/pyro-ld/README.md) for detailed documentation.

## Building

```bash
# Build all components
cargo build --release

# Build specific component
cargo build --release -p pyro-ld

# Run tests
cargo test
```

## Current Implementation Status

### ✅ Completed
- [x] Project structure and workspace setup
- [x] ELF parsing infrastructure (using goblin)
- [x] Basic relocation engine (x86_64)
- [x] Auxiliary vector parsing
- [x] Direct syscall wrappers (exit, write, mmap, munmap)
- [x] Compiler intrinsics (memcpy, memset, etc.)
- [x] Static linking with no_std
- [x] C header generation (cbindgen)
- [x] **Memory allocator (mmap-based bump allocator with 64KB chunks)**

### 🚧 In Progress / TODO
- [ ] File loading via mmap (open/read syscalls)
- [ ] Symbol resolution with hash tables
- [ ] DT_NEEDED recursive library loading
- [ ] RPATH/RUNPATH search paths
- [ ] TLS (Thread-Local Storage) support
- [ ] Additional relocation types
- [ ] Support for architectures beyond x86_64

## Architecture Decisions

### no_std Approach
The dynamic linker uses `no_std` for maximum control and minimal dependencies, essential for a component that bootstraps before any standard library facilities are available.

### Direct Syscalls
Instead of using a syscall wrapper library, we implement syscalls directly via inline assembly for minimal overhead and dependencies.

### Statically-Linked Linker
The dynamic linker itself is statically linked to avoid chicken-and-egg problems during bootstrap.

## Design Considerations

### Separate ELF Crate?
Currently ELF utilities are embedded in `pyro-ld`. As the project grows, these could be extracted to a shared `pyro-elf` crate for reuse by other libc components.

### TLS Support
The current implementation skips TLS. Adding TLS support requires:
- PT_TLS segment parsing
- Thread control block (TCB) allocation  
- Setting %fs/%gs segment registers
- Implementing TLS access models (Local-Exec, Initial-Exec, etc.)

### Standard Library Usage
Committed to `no_std` for runtime components. Build scripts and tests can use `std`.

## Testing

```bash
# Build the linker
cargo build --release -p pyro-ld

# The linker binary is at:
./target/release/ld-pyro

# Currently the linker can bootstrap itself but does not yet
# load external programs or libraries.
```

To test with a simple program (future capability):
```bash
clang -o test test.c -Wl,--dynamic-linker,./target/release/ld-pyro
./test
```

## Dependencies

- **goblin** (0.8): ELF parsing without std
- **scroll** (0.12): Byte buffer reading for ELF structures
- **cbindgen** (0.27): C header generation

## License

TBD

## Contributing

TBD

## Further Reading

- [ELF Specification](https://refspecs.linuxfoundation.org/elf/elf.pdf)
- [x86-64 psABI](https://gitlab.com/x86-psABIs/x86-64-ABI)
- [Linux Boot Protocol](https://www.kernel.org/doc/html/latest/x86/boot.html)
- [How Programs Get Run](https://lwn.net/Articles/631631/)
