// SPDX-License-Identifier: Apache-2.0
// libsai-teralynx-shim.so, loaded RTLD_GLOBAL by ifcs_lib.cpp from libsai's
// own directory before the vendor libifcs.so 0.12.41:
//  * CPython 2.7: libifcs references 19 functions (its CLI/python hooks; all
//    PLT calls, no data symbols), so dlopen(RTLD_NOW) fails without a
//    libpython. syncd has no Python and must not get the vendor's python2.7:
//    these stubs log once and fail every call (NULL / -1).
//  * tl_devmem_memcpy: libifcs memcpy()s to and from the ASIC's PCI BAR
//    (Device memory). Current glibc's aarch64 memcpy uses unaligned and
//    128-bit SIMD accesses, which fault there (SIGBUS in __memcpy_generic
//    inside ifcs_init, first hardware run 2026-11-16); the vendor's glibc 2.25
//    did not. ifcs_lib.cpp points libifcs' memcpy import at this copy loop:
//    8-byte accesses when both pointers and the length allow, else 4, else 1.
#include <stdint.h>
#include <stdio.h>
#include <stddef.h>

void* tl_devmem_memcpy(void* dst, const void* src, size_t n)
{
    uintptr_t d = (uintptr_t)dst, s = (uintptr_t)src;
    if (((d | s | n) & 7) == 0) {
        volatile uint64_t* o = (volatile uint64_t*)dst;
        const volatile uint64_t* i = (const volatile uint64_t*)src;
        for (size_t k = 0; k < n / 8; k++) o[k] = i[k];
    } else if (((d | s | n) & 3) == 0) {
        volatile uint32_t* o = (volatile uint32_t*)dst;
        const volatile uint32_t* i = (const volatile uint32_t*)src;
        for (size_t k = 0; k < n / 4; k++) o[k] = i[k];
    } else {
        volatile uint8_t* o = (volatile uint8_t*)dst;
        const volatile uint8_t* i = (const volatile uint8_t*)src;
        for (size_t k = 0; k < n; k++) o[k] = i[k];
    }
    return dst;
}

static int warned;
static void warn(const char* fn)
{
    if (!warned) {
        warned = 1;
        fprintf(stderr, "sai-teralynx: libifcs called %s: no Python in this process (vendor CLI hooks unavailable)\n", fn);
    }
}
#define PTR(name, ...) void* name(__VA_ARGS__) { warn(#name); return NULL; }
#define INT(name, ...) long name(__VA_ARGS__) { warn(#name); return -1; }

PTR(Py_BuildValue, const char* f, ...)
INT(PyCallable_Check, void* o)
PTR(PyErr_Occurred, void)
void PyErr_Print(void) { warn("PyErr_Print"); }
void Py_Finalize(void) { warn("Py_Finalize"); }
INT(PyGILState_Ensure, void)
void PyGILState_Release(long s) { (void)s; warn("PyGILState_Release"); }
PTR(PyImport_Import, void* n)
void Py_Initialize(void) { warn("Py_Initialize"); }
INT(PyInt_AsLong, void* o)
PTR(PyInt_FromLong, long v)
PTR(PyList_GetItem, void* l, long i)
PTR(PyList_New, long n)
INT(PyList_SetItem, void* l, long i, void* o)
INT(PyLong_AsLong, void* o)
PTR(PyObject_CallFunctionObjArgs, void* c, ...)
PTR(PyObject_CallObject, void* c, void* a)
PTR(PyObject_GetAttrString, void* o, const char* n)
PTR(PyUnicodeUCS2_FromString, const char* s)
