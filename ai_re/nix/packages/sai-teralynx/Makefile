# sai-teralynx: libsai.so for the Teralynx 7 (CS8210-32X) = the sai-openbcm
# SAI frontend, unmodified, + our IFCS backend (src/). See README.md.
#   make SAI_INC=<dir with sai.h> FRONTEND=<sai-openbcm/src> IFCS_CTYPES=<ifcs_ctypes.py> \
#        [FRONTEND_TESTS=<sai-openbcm/tests>]
# No link-time dependency on the vendor SDK: libifcs.so is dlopen'ed at init.
# The IFCS declarations (build/gen/ifcs_api*.h) are generated from
# IFCS_CTYPES (README.md "IFCS declarations"; default.nix fetches it).
# Variables: CXX, CC, AR, CXXFLAGS, CFLAGS, LDFLAGS, PREFIX, DESTDIR, PYTHON.
SAI_INC ?= /usr/include/sai
FRONTEND ?=
FRONTEND_TESTS ?=
IFCS_CTYPES ?=
PYTHON ?= python3
PREFIX ?= /usr
CXX ?= g++
CC ?= gcc
CXXFLAGS ?= -O2 -g
CFLAGS ?= -O2 -g
WARN = -Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers

ifeq ($(FRONTEND),)
$(error FRONTEND=<sai-openbcm src dir> is required)
endif
ifneq ($(wildcard src/ifcs_api.h src/ifcs_api_qos.h),)
$(error src/ifcs_api*.h must not exist: they are generated into build/gen (README.md "IFCS declarations"))
endif
ifeq ($(wildcard $(FRONTEND)/sai_api.cpp),)
$(error FRONTEND=$(FRONTEND) does not look like sai-openbcm/src (no sai_api.cpp))
endif

# The frontend's core, listed explicitly (as sai-openbcm's own Makefile does).
# A .cpp in FRONTEND that is in neither list stops the build, so a new
# frontend file is noticed here instead of silently left out.
CORE = sai_api.cpp sai_switch.cpp sai_port.cpp sai_l2.cpp sai_hostif.cpp sai_l3.cpp sai_lag.cpp \
       sai_temp.cpp sai_caps.cpp sai_diag.cpp sai_acl.cpp sai_qos.cpp sai_mirror.cpp sai_hash.cpp sai_extphy.cpp \
       sai_extmacsec.cpp sai_macsec.cpp fake_backend.cpp
# sai_macsec_rust.cpp: the MACsec model in Rust behind sai-openbcm's RUST_MACSEC switch (replaces sai_macsec.cpp there)
# recording_backend.cpp: test-only (the frontend tests' call log), built for build/test_sai below
FRONTEND_NOT_USED = backend_none.cpp openbcm_backend.cpp ext_phy_check.cpp ext_macsec_check.cpp sai_macsec_rust.cpp \
                    recording_backend.cpp
FE_UNKNOWN = $(filter-out $(CORE) $(FRONTEND_NOT_USED),$(notdir $(wildcard $(FRONTEND)/*.cpp)))
ifneq ($(FE_UNKNOWN),)
$(error FRONTEND has .cpp files this Makefile does not list: $(FE_UNKNOWN); add them to CORE or FRONTEND_NOT_USED)
endif

.DEFAULT_GOAL := all

# Generated IFCS declarations: src/ifcs_api*.h.in (names, ours) filled in
# from the ctypes file (values, layouts, signatures) + src/ifcs_overrides.txt.
GEN = build/gen
IFCS_HDRS = $(GEN)/ifcs_api.h $(GEN)/ifcs_api_qos.h
IFCS_TMPL = src/ifcs_api.h.in src/ifcs_api_qos.h.in
$(IFCS_HDRS) &: $(IFCS_TMPL) src/ifcs_overrides.txt tools/gen_ifcs_api.py tools/ifcs_ctypes_model.py $(IFCS_CTYPES)
	@if [ -z "$(IFCS_CTYPES)" ]; then echo "IFCS_CTYPES=<pyctypes/ifcs_ctypes.py> is required (README.md)" >&2; exit 1; fi
	$(PYTHON) -I -B tools/gen_ifcs_api.py --ctypes $(IFCS_CTYPES) --overrides src/ifcs_overrides.txt -o $(GEN) $(IFCS_TMPL)

CPPFLAGS_ALL = -I$(SAI_INC) -Isrc -I$(GEN) -I$(FRONTEND)
# Our sources: every src/*.cpp (the backend areas tl_*.cpp, the loader, the
# config parsers), so a new area file needs no Makefile change.
OURS = $(basename $(notdir $(wildcard src/*.cpp)))
FE_OBJS = $(patsubst %.cpp,build/fe/%.o,$(CORE)) build/fe/temp_files.o
OBJS = $(FE_OBJS) $(patsubst %,build/%.o,$(OURS))

all: build/libsai.so.1.0.0 build/libsai-teralynx-shim.so build/openbcm-diag build/sai-teralynx-smoke

build/fe/%.o: $(FRONTEND)/%.cpp $(wildcard $(FRONTEND)/*.h)
	@mkdir -p build/fe
	$(CXX) -std=c++17 -fPIC $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -c $< -o $@

build/fe/temp_files.o: $(FRONTEND)/temp_files.c $(FRONTEND)/temp_files.h
	@mkdir -p build/fe
	$(CC) -std=gnu11 -fPIC $(WARN) $(CFLAGS) -I$(FRONTEND) -c $< -o $@

build/%.o: src/%.cpp $(wildcard src/*.h) $(IFCS_HDRS) $(wildcard $(FRONTEND)/*.h)
	@mkdir -p build
	$(CXX) -std=c++17 -fPIC $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -c $< -o $@

build/libsai.so.1.0.0: $(OBJS) $(FRONTEND)/libsai.map
	$(CXX) -shared -fPIC -Wl,-z,defs -Wl,-soname,libsai.so.1 -Wl,--version-script=$(FRONTEND)/libsai.map $(LDFLAGS) \
	  -o $@ $(OBJS) -ldl -lpthread
	ln -sf libsai.so.1.0.0 build/libsai.so.1
	ln -sf libsai.so.1 build/libsai.so

build/libsai.a: $(OBJS)
	$(AR) rcs $@ $^

# Client of SAI_OPENBCM_DIAG_SOCKET (the frontend's diag socket; our backend
# answers a few read-only commands).
build/openbcm-diag: $(FRONTEND)/openbcm_diag.c
	@mkdir -p build
	$(CC) -std=gnu11 -Wall -Wextra $(CFLAGS) $(LDFLAGS) -o $@ $<

# Loaded before libifcs: CPython stubs + a Device-memory-safe memcpy (src/shim.c)
# + the learn-ring read guard (src/ifcs_guard.c).
build/libsai-teralynx-shim.so: src/shim.c src/ifcs_guard.c
	@mkdir -p build
	$(CC) -std=gnu11 -fPIC -shared -Wall -fno-builtin -fno-tree-loop-distribute-patterns $(CFLAGS) $(LDFLAGS) -Wl,-soname,libsai-teralynx-shim.so -o $@ $^ -lpthread

# Direct libsai driver for first runs on the chip (tools/sai_smoke.cpp).
build/sai-teralynx-smoke: tools/sai_smoke.cpp build/libsai.so.1.0.0
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) -I$(SAI_INC) $(LDFLAGS) -o $@ $< build/libsai.so.1.0.0 -Wl,-rpath,$(PREFIX)/lib -lpthread

# ---- tests (native only)
# The mock libifcs: one file per area (tests/mock/mock_<area>.c), every
# IFCS_SYMBOLS entry defined with its declared type (mock.h), none missing
# (--no-undefined). The test: main + one file per area (tests/t_<area>.cpp).
# Both include the generated build/gen/ifcs_api.h.
MOCK_SRCS = $(wildcard tests/mock/*.c)
TEST_SRCS = tests/test_ifcs.cpp $(wildcard tests/t_*.cpp)
build/mock/libifcs.so: $(MOCK_SRCS) tests/mock/mock.h $(IFCS_HDRS)
	@mkdir -p build/mock
	$(CC) -std=gnu11 -fPIC -shared $(WARN) -Werror=incompatible-pointer-types $(CFLAGS) -Isrc -I$(GEN) -Itests/mock \
	  -Wl,--no-undefined -o $@ $(MOCK_SRCS) -lpthread

build/test_ifcs: $(TEST_SRCS) tests/harness.h build/libsai.so.1.0.0
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) -I$(SAI_INC) -Itests -o $@ $(TEST_SRCS) build/libsai.so.1.0.0 -Wl,-rpath,'$$ORIGIN' -ldl -lpthread

# The shim (next to libsai, found by dladdr) is part of the test: t_learn.cpp tests its guard.
TEST_BINS = build/test_ifcs build/mock/libifcs.so build/libsai-teralynx-shim.so
ifneq ($(FRONTEND_TESTS),)
# sai-openbcm's own unit test, unchanged, on this library's objects (frontend
# + fake backend sanity; our backend is only constructed, never initialised):
# test_sai.cpp sees the SAI headers and its test port header only, the port
# (test_port_cpp.cpp, + the frontend's recording_backend.cpp) the frontend's
# internals; test_units.cpp: the frontend's pure C++ units.
FE_TEST_HDRS = $(wildcard $(FRONTEND_TESTS)/*.h $(FRONTEND_TESTS)/*.inc)
build/test_sai.o: $(FRONTEND_TESTS)/test_sai.cpp $(FE_TEST_HDRS)
	@mkdir -p build
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) -I$(SAI_INC) -I$(FRONTEND_TESTS) -c $< -o $@
build/test_port_cpp.o: $(FRONTEND_TESTS)/test_port_cpp.cpp $(FE_TEST_HDRS) $(wildcard $(FRONTEND)/*.h)
	@mkdir -p build
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -I$(FRONTEND_TESTS) -c $< -o $@
build/test_sai: build/test_sai.o build/test_port_cpp.o build/fe/recording_backend.o build/libsai.a
	$(CXX) -std=c++17 $(CXXFLAGS) -o $@ $^ -ldl -lpthread
build/test_units: $(FRONTEND_TESTS)/test_units.cpp $(FE_TEST_HDRS) build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -I$(FRONTEND_TESTS) -o $@ $< build/libsai.a -ldl -lpthread
TEST_BINS += build/test_sai build/test_units
endif

tests: $(TEST_BINS)

check: tests
	./build/test_ifcs $(abspath build/mock/libifcs.so) $(abspath build/check-tmp)
	if [ -x build/test_sai ]; then ./build/test_sai; fi
	if [ -x build/test_units ]; then ./build/test_units; fi

install: build/libsai.so.1.0.0 build/libsai-teralynx-shim.so build/openbcm-diag build/sai-teralynx-smoke
	install -Dm755 build/libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1.0.0
	ln -sf libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1
	ln -sf libsai.so.1 $(DESTDIR)$(PREFIX)/lib/libsai.so
	install -Dm755 build/libsai-teralynx-shim.so $(DESTDIR)$(PREFIX)/lib/libsai-teralynx-shim.so
	install -Dm755 build/openbcm-diag $(DESTDIR)$(PREFIX)/bin/openbcm-diag
	install -Dm755 build/sai-teralynx-smoke $(DESTDIR)$(PREFIX)/bin/sai-teralynx-smoke

clean:
	rm -rf build

.PHONY: all tests check install clean
