# sai-openbcm: libsai.so on the Broadcom OpenBCM SDK (or the fake backend).
#   make SAI_INC=<dir with sai.h>                      fake-only libsai + tests
#   make SAI_INC=... OPENBCM=<libsdk prefix>           + openbcm backend
#   make ... RUST_PHY_SEQ=<prefix>                     the external PHY / MACsec table parsers and the sequence
#                                                      executor from the Rust crate phy-seq instead of the C++ ones
#                                                      (<prefix>/lib/libphy_seq_capi.a, <prefix>/include/phy_seq.h:
#                                                      rust/sai/nix/phy-seq.nix). Same structs, same tests.
#   make ... RUST_MACSEC=<prefix>                      the MACsec object model from the Rust crate sai-macsec:
#                                                      src/sai_macsec_rust.cpp instead of src/sai_macsec.cpp
#                                                      (<prefix>/lib/libsai_macsec_capi.a, <prefix>/include/sai_macsec_rs.h:
#                                                      rust/sai/nix/sai-macsec-capi.nix). Independent of RUST_PHY_SEQ (the
#                                                      engine stays the build's choice); its library also carries phy_seq.h's
#                                                      functions, so with both only it is linked (two Rust static libraries
#                                                      would each bring std): RUST_PHY_SEQ may then name the same prefix.
# Variables: CXX, CC, CXXFLAGS, CFLAGS, LDFLAGS, PREFIX, DESTDIR.
# Exports are controlled by src/libsai.map (sai_* only), not by visibility.
SAI_INC ?= /usr/include/sai
OPENBCM ?=
PREFIX ?= /usr
CXX ?= g++
CC ?= gcc
CXXFLAGS ?= -O2 -g
CFLAGS ?= -O2 -g
WARN = -Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers

SAI_CPPFLAGS = -I$(SAI_INC) -Isrc
CORE = src/sai_api.cpp src/sai_switch.cpp src/sai_port.cpp src/sai_l2.cpp src/sai_hostif.cpp \
       src/sai_l3.cpp src/sai_lag.cpp src/sai_temp.cpp src/sai_caps.cpp src/sai_diag.cpp src/sai_acl.cpp \
       src/sai_qos.cpp src/sai_mirror.cpp src/sai_hash.cpp src/sai_extphy.cpp src/sai_extmacsec.cpp src/sai_macsec.cpp \
       src/fake_backend.cpp

ifneq ($(OPENBCM),)
BACKEND_OBJS = build/openbcm_backend.o build/sdk_glue.o
SDK_CFLAGS = $(shell cat $(OPENBCM)/share/openbcm/cflags) \
  -I$(OPENBCM)/include/openbcm/include -I$(OPENBCM)/include/openbcm/bde \
  -I$(OPENBCM)/include/openbcm/kmod -I$(OPENBCM)/include/openbcm/phymod
SDK_LIBS = -L$(OPENBCM)/lib -lsdk -Wl,-rpath,$(OPENBCM)/lib
TOOLS = build/openbcm-asic-temp build/openbcm-diag build/openbcm-mdio-script build/openbcm-ext-phy build/openbcm-ext-macsec
MDIO_SDK = -DOB_HAVE_SDK build/sdk_glue.o build/temp_files.o $(SDK_LIBS)
MDIO_DEPS = build/sdk_glue.o build/temp_files.o
else
BACKEND_OBJS = build/backend_none.o
SDK_LIBS =
TOOLS = build/openbcm-diag build/openbcm-mdio-script build/openbcm-ext-phy build/openbcm-ext-macsec
MDIO_SDK =
MDIO_DEPS =
endif

RUST_PHY_SEQ ?=
ifneq ($(RUST_PHY_SEQ),)
SAI_CPPFLAGS += -DOB_RUST_PHY_SEQ -I$(RUST_PHY_SEQ)/include
# a Rust static library carries std; what it needs from the system: libc, libgcc_s (unwinding), libdl, libpthread.
# --gc-sections: the archive member is all of std's code the crate could reach, one section per function;
# the link keeps what libsai's seven entry points use.
RUST_LIBS = $(RUST_PHY_SEQ)/lib/libphy_seq_capi.a -ldl -Wl,--gc-sections
else
RUST_LIBS =
endif

RUST_MACSEC ?=
ifneq ($(RUST_MACSEC),)
SAI_CPPFLAGS += -DOB_RUST_MACSEC -I$(RUST_MACSEC)/include
RUST_LIBS = $(RUST_MACSEC)/lib/libsai_macsec_capi.a -ldl -Wl,--gc-sections
CORE := $(filter-out src/sai_macsec.cpp,$(CORE)) src/sai_macsec_rust.cpp
endif

OBJS = $(patsubst src/%.cpp,build/%.o,$(CORE)) build/temp_files.o $(BACKEND_OBJS)

all: build/libsai.so.1.0.0 build/test_sai build/test_extmacsec_trace build/test_mdio_script $(TOOLS)

build/%.o: src/%.cpp src/*.h
	@mkdir -p build
	$(CXX) -std=c++17 -fPIC $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) -c $< -o $@

# Temperature files (fand / Prometheus), shared by libsai and openbcm-asic-temp.
build/temp_files.o: src/temp_files.c src/temp_files.h
	@mkdir -p build
	$(CC) -std=gnu11 -fPIC $(WARN) $(CFLAGS) -Isrc -c $< -o $@

# The SDK glue is C, compiled with exactly the SDK's own defines.
build/sdk_glue.o: src/sdk_glue.c src/sdk_glue.h
	@mkdir -p build
	$(CC) -std=gnu11 -fPIC -Wall -Wno-unused-function $(CFLAGS) $(SDK_CFLAGS) -Isrc -c $< -o $@

# ASIC die temperature reader (attach only), see src/asic_temp.c.
build/openbcm-asic-temp: src/asic_temp.c src/sdk_glue.h src/temp_files.h build/sdk_glue.o build/temp_files.o
	$(CC) -std=gnu11 -Wall -Wextra $(CFLAGS) -Isrc $(LDFLAGS) -o $@ src/asic_temp.c build/sdk_glue.o build/temp_files.o $(SDK_LIBS) -lpthread

# Client of SAI_OPENBCM_DIAG_SOCKET (no SDK code; also in fake builds).
build/openbcm-diag: src/openbcm_diag.c
	@mkdir -p build
	$(CC) -std=gnu11 -Wall -Wextra $(CFLAGS) $(LDFLAGS) -o $@ $<

# MDIO script runner for PHYs without an SDK driver (src/mdio_script.h,
# src/mdio_report.h); without the SDK: --check and --verify-only through
# syncd's diag socket.
MDIO_SRC = src/mdio_script.c src/mdio_report.c
MDIO_HDR = src/mdio_script.h src/mdio_report.h
build/openbcm-mdio-script: src/mdio_script_main.c $(MDIO_SRC) $(MDIO_HDR) src/sdk_glue.h src/temp_files.h $(MDIO_DEPS)
	@mkdir -p build
	$(CC) -std=gnu11 -Wall -Wextra $(CFLAGS) -Isrc $(LDFLAGS) -o $@ src/mdio_script_main.c $(MDIO_SRC) $(MDIO_SDK) -lpthread

build/test_mdio_script: tests/test_mdio_script.c $(MDIO_SRC) $(MDIO_HDR)
	@mkdir -p build
	$(CC) -std=gnu11 -Wall -Wextra $(CFLAGS) -Isrc -o $@ tests/test_mdio_script.c $(MDIO_SRC) -lpthread

# External PHY table checker (src/ext_phy.h): the parser libsai uses.
build/openbcm-ext-phy: src/ext_phy_check.cpp src/ext_phy.h build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) $(LDFLAGS) -o $@ src/ext_phy_check.cpp build/libsai.a $(RUST_LIBS) $(SDK_LIBS) -lpthread

# External MACsec table checker / renderer (src/ext_macsec.h): the parser and executor libsai uses.
build/openbcm-ext-macsec: src/ext_macsec_check.cpp src/ext_macsec.h build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) $(LDFLAGS) -o $@ src/ext_macsec_check.cpp build/libsai.a $(RUST_LIBS) $(SDK_LIBS) -lpthread

build/libsai.a: $(OBJS)
	$(AR) rcs $@ $^

build/libsai.so.1.0.0: $(OBJS) src/libsai.map
	$(CXX) -shared -fPIC -Wl,-z,defs -Wl,-soname,libsai.so.1 -Wl,--version-script=src/libsai.map $(LDFLAGS) \
	  -o $@ $(OBJS) $(RUST_LIBS) $(SDK_LIBS) -lpthread
	ln -sf libsai.so.1.0.0 build/libsai.so.1
	ln -sf libsai.so.1 build/libsai.so

build/test_sai: tests/test_sai.cpp tests/test_acl.inc tests/test_qos.inc tests/test_extphy.inc tests/test_macsec.inc build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) -o $@ tests/test_sai.cpp build/libsai.a $(RUST_LIBS) $(SDK_LIBS) -lpthread

# The sequence trace (ext_macsec.h "extmacsec trace") against a golden text: the same text for
# the C++ and the Rust engine, and no key byte in it.
build/test_extmacsec_trace: tests/test_extmacsec_trace.cpp src/ext_macsec.h build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) -o $@ tests/test_extmacsec_trace.cpp build/libsai.a $(RUST_LIBS) $(SDK_LIBS) -lpthread

# test_mdio_script also runs the tool itself (parse failures, status files,
# --verify-only against a fake diag socket).
check: build/test_sai build/test_extmacsec_trace build/test_mdio_script build/openbcm-mdio-script
	./build/test_sai
	./build/test_extmacsec_trace
	./build/test_mdio_script build/openbcm-mdio-script

install: build/libsai.so.1.0.0 $(TOOLS)
	install -Dm755 build/libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1.0.0
	for t in $(TOOLS); do install -Dm755 $$t $(DESTDIR)$(PREFIX)/bin/$$(basename $$t); done
	ln -sf libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1
	ln -sf libsai.so.1 $(DESTDIR)$(PREFIX)/lib/libsai.so

clean:
	rm -rf build

.PHONY: all check install clean
