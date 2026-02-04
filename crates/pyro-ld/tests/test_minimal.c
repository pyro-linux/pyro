// Minimal test program that can be used to verify the linker
// This file demonstrates what a test would look like

// Build with: gcc -nostdlib -static -o test_minimal test_minimal.c
// Or use with dynamic linker: gcc -o test_dynamic test_minimal.c

void _start() {
  // Minimal exit syscall
  asm volatile("mov $60, %%rax\n"
               "mov $0, %%rdi\n"
               "syscall"
               :
               :
               : "rax", "rdi");
}
