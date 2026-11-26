/* SPDX-License-Identifier: Apache-2.0
 * Test fixture of elf-got (tests/redirect.rs), linked with -z lazy (build.rs): got_call() calls
 * got_target() through this object's PLT (a JUMP_SLOT relocation in .rela.plt, a writable
 * .got.plt page; got_target is exported with default visibility, so the compiler may not bind
 * the call locally), got_addr() reads got_addr_target's address through the GOT (a GLOB_DAT
 * relocation in .rela.dyn, a RELRO page: read-only after relocation). Redirecting a symbol's
 * slots changes what these answer. */
int got_target(int x);
int got_target(int x) { return x + 1; }

int got_call(int x);
int got_call(int x) { return got_target(x) * 2; }

int got_addr_target(int x);
int got_addr_target(int x) { return x + 2; }

typedef int (*got_fn)(int);
got_fn got_addr(void);
got_fn got_addr(void) { return &got_addr_target; }
