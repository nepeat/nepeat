# sai-openbcm: libsai.so on the Broadcom OpenBCM SDK (or the fake backend).
#   make SAI_INC=<dir with sai.h>                      fake-only libsai + tests
#   make SAI_INC=... OPENBCM=<libsdk prefix>           + openbcm backend
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
       src/sai_l3.cpp src/sai_caps.cpp src/fake_backend.cpp

ifneq ($(OPENBCM),)
BACKEND_OBJS = build/openbcm_backend.o build/sdk_glue.o
SDK_CFLAGS = $(shell cat $(OPENBCM)/share/openbcm/cflags) \
  -I$(OPENBCM)/include/openbcm/include -I$(OPENBCM)/include/openbcm/bde \
  -I$(OPENBCM)/include/openbcm/kmod -I$(OPENBCM)/include/openbcm/phymod
SDK_LIBS = -L$(OPENBCM)/lib -lsdk -Wl,-rpath,$(OPENBCM)/lib
else
BACKEND_OBJS = build/backend_none.o
SDK_LIBS =
endif

OBJS = $(patsubst src/%.cpp,build/%.o,$(CORE)) $(BACKEND_OBJS)

all: build/libsai.so.1.0.0 build/test_sai

build/%.o: src/%.cpp src/*.h
	@mkdir -p build
	$(CXX) -std=c++17 -fPIC $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) -c $< -o $@

# The SDK glue is C, compiled with exactly the SDK's own defines.
build/sdk_glue.o: src/sdk_glue.c src/sdk_glue.h
	@mkdir -p build
	$(CC) -std=gnu11 -fPIC -Wall -Wno-unused-function $(CFLAGS) $(SDK_CFLAGS) -Isrc -c $< -o $@

build/libsai.a: $(OBJS)
	$(AR) rcs $@ $^

build/libsai.so.1.0.0: $(OBJS) src/libsai.map
	$(CXX) -shared -fPIC -Wl,-z,defs -Wl,-soname,libsai.so.1 -Wl,--version-script=src/libsai.map $(LDFLAGS) \
	  -o $@ $(OBJS) $(SDK_LIBS) -lpthread
	ln -sf libsai.so.1.0.0 build/libsai.so.1
	ln -sf libsai.so.1 build/libsai.so

build/test_sai: tests/test_sai.cpp build/libsai.a
	$(CXX) -std=c++17 $(WARN) $(CXXFLAGS) $(SAI_CPPFLAGS) -o $@ tests/test_sai.cpp build/libsai.a $(SDK_LIBS) -lpthread

check: build/test_sai
	./build/test_sai

install: build/libsai.so.1.0.0
	install -Dm755 build/libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1.0.0
	ln -sf libsai.so.1.0.0 $(DESTDIR)$(PREFIX)/lib/libsai.so.1
	ln -sf libsai.so.1 $(DESTDIR)$(PREFIX)/lib/libsai.so

clean:
	rm -rf build

.PHONY: all check install clean
