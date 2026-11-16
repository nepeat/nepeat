# sai-teralynx: libsai.so for the Teralynx 7 (CS8210-32X) = the sai-openbcm
# SAI frontend, unmodified, + our IFCS backend (src/). See README.md.
#   make SAI_INC=<dir with sai.h> FRONTEND=<sai-openbcm/src> [FRONTEND_TESTS=<sai-openbcm/tests>]
# No link-time dependency on the vendor SDK: libifcs.so is dlopen'ed at init.
# Variables: CXX, CC, AR, CXXFLAGS, CFLAGS, LDFLAGS, PREFIX, DESTDIR.
SAI_INC ?= /usr/include/sai
FRONTEND ?=
FRONTEND_TESTS ?=
PREFIX ?= /usr
CXX ?= g++
CC ?= gcc
CXXFLAGS ?= -O2 -g
CFLAGS ?= -O2 -g
WARN = -Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers

ifeq ($(FRONTEND),)
$(error FRONTEND=<sai-openbcm src dir> is required)
endif
ifeq ($(wildcard $(FRONTEND)/sai_api.cpp),)
$(error FRONTEND=$(FRONTEND) does not look like sai-openbcm/src (no sai_api.cpp))
endif

# The frontend's core, listed explicitly (as sai-openbcm's own Makefile does).
# A .cpp in FRONTEND that is in neither list stops the build, so a new
# frontend file is noticed here instead of silently left out.
CORE = sai_api.cpp sai_switch.cpp sai_port.cpp sai_l2.cpp sai_hostif.cpp sai_l3.cpp sai_lag.cpp \
       sai_temp.cpp sai_caps.cpp sai_diag.cpp sai_acl.cpp fake_backend.cpp
FRONTEND_NOT_USED = backend_none.cpp openbcm_backend.cpp
FE_UNKNOWN = $(filter-out $(CORE) $(FRONTEND_NOT_USED),$(notdir $(wildcard $(FRONTEND)/*.cpp)))
ifneq ($(FE_UNKNOWN),)
$(error FRONTEND has .cpp files this Makefile does not list: $(FE_UNKNOWN); add them to CORE or FRONTEND_NOT_USED)
endif

CPPFLAGS_ALL = -I$(SAI_INC) -Isrc -I$(FRONTEND)
OURS = ifcs_backend ifcs_config ifcs_lib
FE_OBJS = $(patsubst %.cpp,build/fe/%.o,$(CORE)) build/fe/temp_files.o
OBJS = $(FE_OBJS) $(patsubst %,build/%.o,$(OURS))

all: build/libsai.so.1.0.0 build/libsai-teralynx-shim.so build/openbcm-diag build/sai-teralynx-smoke

build/fe/%.o: $(FRONTEND)/%.cpp $(wildcard $(FRONTEND)/*.h)
	@mkdir -p build/fe
	$(CXX) -std=c++17 -fPIC $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -c $< -o $@

build/fe/temp_files.o: $(FRONTEND)/temp_files.c $(FRONTEND)/temp_files.h
	@mkdir -p build/fe
	$(CC) -std=gnu11 -fPIC $(WARN) $(CFLAGS) -I$(FRONTEND) -c $< -o $@

build/%.o: src/%.cpp $(wildcard src/*.h) $(wildcard $(FRONTEND)/*.h)
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

# Loaded before libifcs: CPython stubs + a Device-memory-safe memcpy (src/shim.c).
build/libsai-teralynx-shim.so: src/shim.c
	@mkdir -p build
	$(CC) -std=gnu11 -fPIC -shared -Wall -fno-builtin -fno-tree-loop-distribute-patterns $(CFLAGS) $(LDFLAGS) -Wl,-soname,libsai-teralynx-shim.so -o $@ $<

# Direct libsai driver for first runs on the chip (tools/sai_smoke.cpp).
build/sai-teralynx-smoke: tools/sai_smoke.cpp build/libsai.so.1.0.0
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) -I$(SAI_INC) $(LDFLAGS) -o $@ $< build/libsai.so.1.0.0 -Wl,-rpath,$(PREFIX)/lib -lpthread

# ---- tests (native only)
build/mock/libifcs.so: tests/mock_ifcs.c src/ifcs_api.h
	@mkdir -p build/mock
	$(CC) -std=gnu11 -fPIC -shared $(WARN) -Werror=incompatible-pointer-types $(CFLAGS) -Isrc -o $@ $< -lpthread

build/test_ifcs: tests/test_ifcs.cpp build/libsai.so.1.0.0
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) -I$(SAI_INC) -o $@ $< build/libsai.so.1.0.0 -Wl,-rpath,'$$ORIGIN' -ldl -lpthread

TEST_BINS = build/test_ifcs build/mock/libifcs.so
ifneq ($(FRONTEND_TESTS),)
# sai-openbcm's own unit test, unchanged, on this library's objects (frontend
# + fake backend sanity; our backend is only constructed, never initialised).
build/test_sai: $(FRONTEND_TESTS)/test_sai.cpp $(FRONTEND_TESTS)/test_acl.inc build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(CPPFLAGS_ALL) -o $@ $< build/libsai.a -ldl -lpthread
TEST_BINS += build/test_sai
endif

tests: $(TEST_BINS)

check: tests
	./build/test_ifcs $(abspath build/mock/libifcs.so) $(abspath build/check-tmp)
	if [ -x build/test_sai ]; then ./build/test_sai; fi

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
