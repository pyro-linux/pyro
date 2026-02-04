# pyro-ld - Dynamic Linker for Pyro Libc

A foundational dynamic linker (ld.so) implementation in Rust for the Pyro libc project.

## Overview

`pyro-ld` is a minimal ELF dynamic linker that can:
- Bootstrap itself from the Linux kernel
- Parse ELF headers and program segments
- Apply relocations (R_X86_64_RELATIVE, R_X86_64_GLOB_DAT, R_X86_64_JUMP_SLOT)
- Transfer control to loaded programs

## Architecture

The linker is built as a `no_std` binary that receives control directly from the Linux kernel. The entry point parses the initial stack to extract:
- argc/argv/envp
- Auxiliary vectors (AT_PHDR, AT_ENTRY, AT_BASE, etc.)

### Modules

- **startup**: Entry point and auxiliary vector parsing
- **elf**: ELF file parsing using goblin
- **relocation**: Relocation processing engine
- **linker**: Main linker logic and object management
- **syscall**: Direct syscall implementations (exit, write, mmap, munmap)
- **allocator**: Bump allocator using mmap (64KB chunks)
- **intrinsics**: Compiler builtins (memcpy, memset, memmove, memcmp)

## Building

```bash
cargo build --release -p pyro-ld
```

The output binary `ld-pyro` can be used as an ELF interpreter.

## Testing

Create a simple test program:

```c
// test.c
#include <stdio.h>

int main() {
    printf("Hello from pyro-ld!\n");
    return 0;
}
```

Build with pyro-ld as the interpreter:

```bash
clang -o test test.c -Wl,--dynamic-linker,./target/release/ld-pyro
./test
```

## Current Limitations

1. **No external library loading**: Can only relocate itself, cannot load libc.so or other dependencies
2. **No symbol resolution**: Symbol lookup across objects not implemented
3. **No DT_NEEDED processing**: Cannot handle library dependencies
4. **No TLS support**: Thread-local storage not implemented
5. **x86_64 only**: Only supports x86_64 relocations
6. **Bump allocator only**: No individual deallocation (memory freed on process exit)

## Next Steps

1. ~~Implement proper bump/slab allocator~~ ✅ **Done - mmap-based bump allocator**
2. Add file loading via mmap (open/read syscalls)
3. Implement symbol resolution with hash tables
4. Support DT_NEEDED recursive loading
5. Add RPATH/RUNPATH search path support
6. Implement TLS initialization
7. Support additional relocation types
8. Add support for other architectures

## Design Decisions

### no_std Approach

The linker uses `no_std` for maximum control and minimal dependencies. This is essential for a component that must bootstrap before any standard library facilities are available.

### Direct Syscalls

Direct syscall implementations via inline assembly provide minimal overhead and avoid external dependencies. Currently implemented:
- `exit` (SYS_exit = 60)
- `write` (SYS_write = 1)
- `mmap` (SYS_mmap = 9)
- `munmap` (SYS_munmap = 11)

### Bump Allocator with mmap

The allocator uses a simple bump allocation strategy:
- Allocates memory in 64KB chunks via `mmap`
- Thread-safe using atomic operations (`AtomicUsize`)
- Automatic alignment to 16 bytes minimum
- No individual deallocation (memory freed on process exit)
- Grows automatically when more memory is needed

This approach is ideal for a dynamic linker because:
- Simple and fast allocation
- Minimal bookkeeping overhead
- No fragmentation issues
- Memory is only needed during program startup

### goblin for ELF Parsing

The `goblin` crate handles ELF format details reliably in a no_std environment.

## Further Considerations

### Separate pyro-elf Crate?

Currently ELF utilities are in pyro-ld. As the project grows, extracting to a shared `pyro-elf` crate would allow reuse by libc components.

### TLS Support

The current implementation skips TLS. Adding TLS support requires:
- PT_TLS segment parsing
- Thread control block (TCB) allocation
- Setting %fs/%gs segment registers
- Implementing TLS access models (Local-Exec, Initial-Exec, etc.)

### Full no_std vs Partial std

Committed to `no_std` for the dynamic linker proper. Tests and build scripts can use std.
