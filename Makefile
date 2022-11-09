PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/RELEASEX64_OVMF.fd
QEMU=qemu-system-x86_64
PORT_MONITOR?=2222

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=net1 -device rtl8139,netdev=net1 \
		-object filter-dump,id=f2,netdev=net1,file=dump_net1.dat \
		-m 1024M \
		-drive format=raw,file=fat:rw:mnt \
		-chardev file,id=char_com1,mux=on,path=com1.log \
		-chardev stdio,id=char_com2,mux=on,logfile=com2.log \
		-serial chardev:char_com1 \
		-serial chardev:char_com2 \
		-rtc base=localtime \
		-monitor telnet:0.0.0.0:$(PORT_MONITOR),server,nowait \
		--no-reboot \
		${MORE_QEMU_FLAGS}

# -device usb-host,hostbus=1,hostport=1 \
# -device usb-host,hostbus=2,hostport=1.2.1 \
# -device usb-mouse \
# -netdev user,id=net0 -device usb-net,netdev=net0 \
# -object filter-dump,id=f1,netdev=net0,file=dump_net0.dat \

HOST_TARGET=`rustc -V -v | grep 'host:' | sed 's/host: //'`

default: bin

.PHONY: \
	app \
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

CLIPPY_OPTIONS=-D warnings

clippy: font
	cd font && cargo clippy -- ${CLIPPY_OPTIONS}
	cd os && cargo clippy -- ${CLIPPY_OPTIONS}
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
	git add .
	make filecheck
	rustup component add rustfmt
	rustup component add clippy
	cargo fmt
	make rustcheck
	make clippy
	make spellcheck
	make # build
	make test
	./scripts/ensure_objs_are_not_under_git_control.sh
	git diff HEAD --color=always | less -R
	git commit

font:
	mkdir -p generated
	cargo run --bin font font/font.txt > generated/font.rs

filecheck:
	@! git ls-files | grep -v -E '(\.(rs|md|toml|sh|txt|json|lock)|Makefile|LICENSE|rust-toolchain|Dockerfile|OVMF.fd|\.yml|\.gitignore)$$' \
		|| ! echo "!!! Unknown file type is being added! Do you really want to commit the file above? (if so, just modify filecheck recipe)"

spellcheck :
	@scripts/spellcheck.sh recieve receive
	@scripts/spellcheck.sh faild failed
	@scripts/spellcheck.sh mappng mapping

rustcheck :
	@scripts/rustcheck.sh

run :
	# cd into os/examples to use internal_launch_qemu recipe instead of internal_run_os_test in scripts/launch_qemu.sh
	cd os/examples && cargo run --release

run_debug :
	# cd into os/examples to use internal_launch_qemu recipe instead of internal_run_os_test in scripts/launch_qemu.sh
	cd os/examples && cargo run

run_example :
	cd os/examples && cargo run --example ch2_show_mmap

run_dbgutil :
	make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1 | cargo run --bin=dbgutil

pxe :
	scp mnt/EFI/BOOT/BOOTX64.EFI deneb:/var/tftp/wasabios

app :
	-rm -r generated/bin
	make -C app/hello

run_deps : app
	-rm -rf mnt
	mkdir -p mnt/
	mkdir -p mnt/EFI/BOOT
	cp README.md mnt/
	cp generated/bin/* mnt/

watch_serial:
	while ! telnet localhost 1235 ; do sleep 1 ; done ;

watch_qemu_monitor:
	while ! telnet localhost ${PORT_MONITOR} ; do sleep 1 ; done ;

install : run_deps
	cd os && cargo install --path . --root ../generated/
	cp  generated/bin/os.efi mnt/EFI/BOOT/BOOTX64.EFI
	@read -p "Write LIUMOS to /Volumes/LIUMOS. Are you sure? [Enter to proceed, or Ctrl-C to abort] " REPLY && \
		cp -r mnt/* /Volumes/LIUMOS/ && diskutil eject /Volumes/LIUMOS/ && echo "install done."

internal_launch_qemu : run_deps
	@echo Using ${PATH_TO_EFI} as a os...
	cp ${PATH_TO_EFI} mnt/EFI/BOOT/BOOTX64.EFI
	$(QEMU) $(QEMU_ARGS)

internal_run_os_test : run_deps
	@echo Using ${LOADER_TEST_EFI} as a os...
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
