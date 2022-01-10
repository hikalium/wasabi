PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/bios64.bin
QEMU=qemu-system-x86_64
DEBUG_BIN_PATH=${PROJECT_ROOT}/target/x86_64-unknown-uefi/debug/loader.efi
PORT_MONITOR?=2222

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci -device usb-mouse \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=usbnet0 -device usb-net,netdev=usbnet0 \
		-object filter-dump,id=f1,netdev=usbnet0,file=dump_usb_nic.dat \
		-m 2G \
		-drive format=raw,file=fat:rw:mnt \
		-chardev file,id=char_com1,mux=on,path=com1.log \
		-chardev stdio,id=char_com2,mux=on,logfile=com2.log \
		-serial chardev:char_com1 \
		-serial chardev:char_com2 \
		-rtc base=localtime \
		-monitor telnet:0.0.0.0:$(PORT_MONITOR),server,nowait

HOST_TARGET=`rustc -V -v | grep 'host:' | sed 's/host: //'`

default: bin

.PHONY: \
	bin \
	commit \
	dump_config \
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

dump_config:
	@echo "Host target: $(HOST_TARGET)"

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

internal_run_loader_test :
	@echo Using ${LOADER_TEST_EFI} as a loader...
	mkdir -p mnt/
	-rm -rf mnt/*
	mkdir -p mnt/EFI/BOOT
	cp ${LOADER_TEST_EFI} mnt/EFI/BOOT/BOOTX64.EFI
	$(QEMU) $(QEMU_ARGS) -display none ; \
		RETCODE=$$? ; \
		if [ $$RETCODE -eq 3 ]; then \
			echo "\nPASS" ; \
			exit 0 ; \
		else \
			echo "\nFAIL: QEMU returned $$RETCODE" ; \
			exit 1 ; \
		fi
