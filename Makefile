PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/bios64.bin
QEMU=qemu-system-x86_64
DEBUG_BIN_PATH=${PROJECT_ROOT}/target/x86_64-unknown-uefi/debug/loader.efi

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci -device usb-mouse \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=usbnet0 -device usb-net,netdev=usbnet0 \
		-object filter-dump,id=f1,netdev=usbnet0,file=dump_usb_nic.dat \
		-m 2G \
		-drive format=raw,file=fat:rw:mnt \
		-serial tcp::1234,server,nowait \
		-serial tcp::1235,server,nowait \
		-rtc base=localtime \
		-monitor stdio

default: bin

.PHONY: \
	bin \
	commit \
	font \
	spellcheck \
	run \
	run_deps \
	watch_serial \
	# Keep this line blank

bin: font
	cd font && cargo build
	cd loader && cargo build

clippy: font
	cd font && cargo clippy -- -D warnings
	cd loader && cargo clippy -- -D warnings

test: font
	cd font && cargo test
	cd loader && cargo test

commit :
	cargo fmt
	make clippy
	make spellcheck
	make # build
	make test
	git add .
	./scripts/ensure_objs_are_not_under_git_control.sh
	git diff HEAD --color=always | less -R
	git commit

font:
	mkdir -p generated
	cargo run --bin font font/font.txt > generated/font.rs

spellcheck :
	@scripts/spellcheck.sh recieve receive

run : run_deps
	$(QEMU) $(QEMU_ARGS)

run_deps :
	make
	mkdir -p mnt/
	-rm -rf mnt/*
	mkdir -p mnt/EFI/BOOT
	cp ${DEBUG_BIN_PATH} mnt/EFI/BOOT/BOOTX64.EFI

watch_serial:
	while ! telnet localhost 1235 ; do sleep 1 ; done ;
