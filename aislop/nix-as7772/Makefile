NIX       ?= nix
TFTP_ROOT ?= /var/lib/tftpboot

.PHONY: all fit kernel initramfs inspect tftp clean help

# ── Default ──────────────────────────────────────────────────────────────────

all: fit

# ── Build targets ─────────────────────────────────────────────────────────────

## FIT image: kernel + DTB + initramfs, ready to TFTP to the switch
fit:
	$(NIX) build .#fitImage
	@echo ""
	@echo "FIT image:  $$(readlink -f result)/as7772-nixos-netboot.itb"
	@ls -lh result/as7772-nixos-netboot.itb

## Kernel only (uImage + DTB + modules)
kernel:
	$(NIX) build .#kernel --out-link result-kernel
	@echo ""
	@ls -lh result-kernel/boot/

## Initramfs only
initramfs:
	$(NIX) build .#initramfs --out-link result-initramfs
	@echo ""
	@ls -lh result-initramfs/

# ── Utility targets ───────────────────────────────────────────────────────────

## Show FIT image structure and component hashes
inspect: fit
	mkimage -l result/as7772-nixos-netboot.itb

## Copy FIT image to TFTP root (requires write access to TFTP_ROOT)
tftp: fit
	install -D -m 644 result/as7772-nixos-netboot.itb \
	    $(TFTP_ROOT)/as7772-nixos-netboot.itb
	@echo "Installed → $(TFTP_ROOT)/as7772-nixos-netboot.itb"

## Remove result symlinks (Nix store entries are unaffected)
clean:
	rm -f result result-kernel result-initramfs

# ── Help ─────────────────────────────────────────────────────────────────────

help:
	@echo "Usage: make [target] [VAR=value ...]"
	@echo ""
	@echo "Build targets:"
	@echo "  fit        FIT image: kernel + DTB + initramfs  (default)"
	@echo "  kernel     Kernel only  → result-kernel/boot/"
	@echo "  initramfs  Initramfs only → result-initramfs/"
	@echo ""
	@echo "Utility targets:"
	@echo "  inspect    Show FIT image structure (mkimage -l)"
	@echo "  tftp       Copy FIT image to TFTP_ROOT"
	@echo "  clean      Remove result symlinks"
	@echo ""
	@echo "Variables:"
	@echo "  TFTP_ROOT  $(TFTP_ROOT)"
	@echo "  NIX        $(NIX)"
