// SPDX-License-Identifier: Apache-2.0
#include "ifcs_lib.h"

#include <dlfcn.h>
#include <elf.h>
#include <link.h>
#include <sys/mman.h>
#include <unistd.h>
#include <cstdio>
#include <cstring>
#include <mutex>

static int tl_anchor;

namespace {

// Point one imported function of an already loaded object (matched by path)
// at fn: rewrite its JUMP_SLOT / GLOB_DAT GOT entries. Returns entries changed.
// The native relocation types, so the x86 test build exercises it on the mock.
#if defined(__aarch64__)
constexpr unsigned kJumpSlot = R_AARCH64_JUMP_SLOT, kGlobDat = R_AARCH64_GLOB_DAT;
#elif defined(__x86_64__)
constexpr unsigned kJumpSlot = R_X86_64_JUMP_SLOT, kGlobDat = R_X86_64_GLOB_DAT;
#else
#error "ifcs_lib.cpp: GOT redirect for this architecture not written"
#endif
struct Redirect { const char* path; const char* sym; void* fn; int changed; };

int redirect_cb(struct dl_phdr_info* info, size_t, void* data)
{
    auto* r = static_cast<Redirect*>(data);
    if (!info->dlpi_name || strcmp(info->dlpi_name, r->path) != 0) return 0;
    const ElfW(Dyn)* dyn = nullptr;
    for (int i = 0; i < info->dlpi_phnum; i++)
        if (info->dlpi_phdr[i].p_type == PT_DYNAMIC)
            dyn = reinterpret_cast<const ElfW(Dyn)*>(info->dlpi_addr + info->dlpi_phdr[i].p_vaddr);
    if (!dyn) return 1;
    const ElfW(Sym)* symtab = nullptr; const char* strtab = nullptr;
    const ElfW(Rela)* rela[2] = {nullptr, nullptr}; size_t relasz[2] = {0, 0};
    for (const ElfW(Dyn)* d = dyn; d->d_tag != DT_NULL; d++) {
        switch (d->d_tag) {
        case DT_SYMTAB: symtab = reinterpret_cast<const ElfW(Sym)*>(d->d_un.d_ptr); break;
        case DT_STRTAB: strtab = reinterpret_cast<const char*>(d->d_un.d_ptr); break;
        case DT_JMPREL: rela[0] = reinterpret_cast<const ElfW(Rela)*>(d->d_un.d_ptr); break;
        case DT_PLTRELSZ: relasz[0] = d->d_un.d_val; break;
        case DT_RELA: rela[1] = reinterpret_cast<const ElfW(Rela)*>(d->d_un.d_ptr); break;
        case DT_RELASZ: relasz[1] = d->d_un.d_val; break;
        }
    }
    if (!symtab || !strtab) return 1;
    long pg = sysconf(_SC_PAGESIZE);
    for (int t = 0; t < 2; t++) {
        if (!rela[t]) continue;
        for (size_t k = 0; k < relasz[t] / sizeof(ElfW(Rela)); k++) {
            const ElfW(Rela)& e = rela[t][k];
            unsigned type = ELF64_R_TYPE(e.r_info);
            if (type != kJumpSlot && type != kGlobDat) continue;
            if (strcmp(strtab + symtab[ELF64_R_SYM(e.r_info)].st_name, r->sym) != 0) continue;
            void** slot = reinterpret_cast<void**>(info->dlpi_addr + e.r_offset);
            uintptr_t page = reinterpret_cast<uintptr_t>(slot) & ~static_cast<uintptr_t>(pg - 1);
            mprotect(reinterpret_cast<void*>(page), pg, PROT_READ | PROT_WRITE);   // RELRO: may be read-only
            *slot = r->fn;
            r->changed++;
        }
    }
    return 1;
}

} // namespace

namespace tl {

bool ifcs_lib_load(const std::string& path, IfcsLib& lib, std::string& err, const std::string& trace,
                   const GuardOptions& guard)
{
    static std::mutex mu;
    std::lock_guard<std::mutex> g(mu);
    if (path.empty() || path[0] != '/') {
        err = "SAI_TERALYNX_LIBIFCS must be an absolute path (got '" + path + "')";
        return false;
    }
    // libifcs needs CPython symbols and a Device-memory-safe memcpy (shim.c),
    // which sits next to libsai.
    void* shim = nullptr;
    Dl_info di;
    if (dladdr(&tl_anchor, &di) && di.dli_fname) {
        std::string d = di.dli_fname;
        d = d.substr(0, d.rfind('/') + 1) + "libsai-teralynx-shim.so";
        shim = dlopen(d.c_str(), RTLD_NOW | RTLD_GLOBAL);   // missing: libifcs' dlopen names the symbol
    }
    void* h = dlopen(path.c_str(), RTLD_NOW | RTLD_GLOBAL);
    if (!h) {
        const char* e = dlerror();
        err = "dlopen " + path + ": " + (e ? e : "unknown error");
        return false;
    }
    if (shim) {
        if (void* fn = dlsym(shim, "tl_devmem_memcpy")) {
            Redirect r{path.c_str(), "memcpy", fn, 0};
            dl_iterate_phdr(redirect_cb, &r);
            if (r.changed == 0)   // e.g. the test mock; the real libifcs.so imports it
                fprintf(stderr, "sai-teralynx: %s has no memcpy import to redirect\n", path.c_str());
        }
    }
    // The learn-ring read guard (ifcs_guard.c): before ifcs_init starts the learn thread.
    if (guard.enable) {
        using init_fn = int (*)(void*, void*, const char*, unsigned, unsigned long long);
        auto init = reinterpret_cast<init_fn>(shim ? dlsym(shim, "tl_ifcs_guard_init") : nullptr);
        auto done = reinterpret_cast<void (*)(int)>(shim ? dlsym(shim, "tl_ifcs_guard_installed") : nullptr);
        void* rd = dlsym(h, "node_pci_reg_read32");
        void* wr = dlsym(h, "node_pci_reg_write32");
        if (!init || !done) {
            fprintf(stderr, "sai-teralynx: no libsai-teralynx-shim.so guard: libifcs' learn-ring reads are NOT guarded\n");
        } else if (!rd || !wr || init(rd, wr, guard.stats_path.c_str(), guard.ring_size, guard.malloc_limit) != 0) {
            fprintf(stderr, "sai-teralynx: %s exports no node_pci_reg_read32/_write32: learn-ring guard not installed\n", path.c_str());
            done(0);
        } else {
            Redirect rr{path.c_str(), "node_pci_reg_read32", dlsym(shim, "tl_ifcs_reg_read32"), 0};
            Redirect rw{path.c_str(), "node_pci_reg_write32", dlsym(shim, "tl_ifcs_reg_write32"), 0};
            Redirect rm{path.c_str(), "im_malloc", dlsym(shim, "tl_ifcs_im_malloc"), 0};
            if (rr.fn && rw.fn && rm.fn) {
                dl_iterate_phdr(redirect_cb, &rr);
                dl_iterate_phdr(redirect_cb, &rw);
                dl_iterate_phdr(redirect_cb, &rm);
            }
            done(rr.changed > 0);
            fprintf(stderr,
                    "sai-teralynx: learn-ring guard %s: GOT entries node_pci_reg_read32 %d, node_pci_reg_write32 %d, "
                    "im_malloc %d (ring %u, im_malloc limit %llu B, stats %s)\n",
                    rr.changed > 0 ? "active" : "NOT active (no node_pci_reg_read32 import)", rr.changed, rw.changed,
                    rm.changed, guard.ring_size, (unsigned long long)guard.malloc_limit,
                    guard.stats_path.empty() ? "none" : guard.stats_path.c_str());
        }
    }
    void* th = nullptr;
    if (!trace.empty()) {
        th = dlopen(trace.c_str(), RTLD_NOW | RTLD_GLOBAL);   // GLOBAL: the SAI script runner finds ifcs_trace_mark
        void* bind = th ? dlsym(th, "ifcs_trace_bind") : nullptr;
        if (!bind) {
            const char* e = dlerror();
            err = "SAI_TERALYNX_IFCS_TRACE " + trace + ": " + (th ? "no ifcs_trace_bind" : (e ? e : "dlopen failed"));
            return false;
        }
        reinterpret_cast<void (*)(void*)>(bind)(h);
        fprintf(stderr, "sai-teralynx: IFCS calls traced by %s\n", trace.c_str());
    }
    IfcsLib l;
    l.handle = h;
    l.path = path;
#define TL_FN_LOAD(n)                                                              \
    do {                                                                           \
        dlerror();                                                                 \
        void* p = th ? dlsym(th, #n) : nullptr;                                    \
        if (!p) p = dlsym(h, #n);                                                  \
        if (!p) {                                                                  \
            err = path + ": missing symbol " #n " (wrong libifcs version?)";       \
            return false;                                                          \
        }                                                                          \
        l.n = reinterpret_cast<n##_fn>(p);                                         \
    } while (0);
    IFCS_SYMBOLS(TL_FN_LOAD)
#undef TL_FN_LOAD
    lib = l;
    return true;
}

} // namespace tl
