obj-m += yacht_boot.o
yacht_boot-y := yacht_boot_main.o yacht_trampoline.o
obj-m += yacht_sram_probe.o

YACHT_JUMP_MODE ?= 0
ccflags-y += -fno-stack-protector -fno-builtin -DYACHT_JUMP_MODE=$(YACHT_JUMP_MODE)
