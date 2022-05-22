PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/RELEASEX64_OVMF.fd
QEMU=qemu-system-x86_64
PORT_MONITOR?=2222

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci -device usb-mouse \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=usbnet0 -device usb-net,netdev=usbnet0 \
		-object filter-dump,id=f1,netdev=usbnet0,file=dump_usb_nic.dat \
		-m 1024M \
		-drive format=raw,file=fat:rw:mnt \
		-chardev file,id=char_com1,mux=on,path=com1.log \
		-chardev stdio,id=char_com2,mux=on,logfile=com2.log \
		-serial chardev:char_com1 \
		-serial chardev:char_com2 \
		-rtc base=localtime \
		-monitor telnet:0.0.0.0:$(PORT_MONITOR),server,nowait \
		${MORE_QEMU_FLAGS}

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
	rustup component add rust-src
	cd font && cargo build
	cd os && cargo build

clippy: font
	cd font && cargo clippy -- -D warnings
	cd os && cargo clippy -- -D warnings
	# Following ones will run clippy on examples as well, but disabled for now
	# cd font && cargo clippy --all-features --all-targets -- -D warnings
	# cd os && cargo clippy --all-features --all-targets -- -D warnings

dump_config:
	@echo "Host target: $(HOST_TARGET)"

test: font
	cd os && cargo test --bin os
	cd os && cargo test --lib
	# For now, only OS tests are run to speed up the development
	# cd os && cargo test -vvv
	# cd os && cargo test --examples
	# cd font && cargo test

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
	# cd into os/examples to use internal_launch_qemu recipe instead of internal_run_os_test in scripts/launch_qemu.sh
	cd os/examples && cargo run

run_release : run_deps
	# cd into os/examples to use internal_launch_qemu recipe instead of internal_run_os_test in scripts/launch_qemu.sh
	cd os/examples && cargo run --release

run_example : run_deps
	cd os/examples && cargo run --example ch2_show_mmap

pxe : run_deps
	scp mnt/EFI/BOOT/BOOTX64.EFI deneb:/var/tftp/wasabios

run_deps :
	make bin
	-rm -rf mnt
	mkdir -p mnt/
	mkdir -p mnt/EFI/BOOT
	cp README.md mnt/

watch_serial:
	while ! telnet localhost 1235 ; do sleep 1 ; done ;

watch_qemu_monitor:
	while ! telnet localhost ${PORT_MONITOR} ; do sleep 1 ; done ;

install : run_deps
	cd os && cargo install --path . --root ../generated/
	cp  generated/bin/os.efi mnt/EFI/BOOT/BOOTX64.EFI
	@read -p "Write LIUMOS to /Volumes/LIUMOS. Are you sure? [Enter to proceed, or Ctrl-C to abort] " && \
		cp -r mnt/* /Volumes/LIUMOS/ && diskutil eject /Volumes/LIUMOS/ && echo "install done."

internal_launch_qemu :
	@echo Using ${PATH_TO_EFI} as a os...
	mkdir -p mnt/
	-rm -rf mnt/*
	mkdir -p mnt/EFI/BOOT
	cp ${PATH_TO_EFI} mnt/EFI/BOOT/BOOTX64.EFI
	$(QEMU) $(QEMU_ARGS)

internal_run_os_test :
	@echo Using ${LOADER_TEST_EFI} as a os...
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

act:
	act -P hikalium/wasabi-builder:latest=hikalium/wasabi-builder:latest
