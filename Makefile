PROJECT_ROOT:=$(dir $(abspath $(lastword $(MAKEFILE_LIST))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/bios64.bin
QEMU=qemu-system-x86_64
DEBUG_BIN_PATH=${PROJECT_ROOT}/target/x86_64-unknown-uefi/debug/wasabi_os.efi

QEMU_ARGS=\
		-machine q35 -cpu qemu64 \
		-bios $(OVMF) \
		-device qemu-xhci -device usb-mouse \
		-netdev user,id=usbnet0 -device usb-net,netdev=usbnet0 \
		-object filter-dump,id=f1,netdev=usbnet0,file=dump_usb_nic.dat \
		-m 2G \
		-drive format=raw,file=fat:rw:mnt \
		-rtc base=localtime

default:
	cargo build

.PHONY: run_deps run

run_deps :
	mkdir -p mnt/
	-rm -rf mnt/*
	mkdir -p mnt/EFI/BOOT
	cp ${DEBUG_BIN_PATH} mnt/EFI/BOOT/BOOTX64.EFI

run : run_deps
	$(QEMU) $(QEMU_ARGS)
