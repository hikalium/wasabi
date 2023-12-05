PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))
OVMF=${PROJECT_ROOT}/third_party/ovmf/RELEASEX64_OVMF.fd
QEMU=qemu-system-x86_64
PORT_MONITOR?=2345
PORT_OFFSET_VNC?=5
VNC_PASSWORD?=wasabi
INIT?=hello1
RUNNER_NORMAL=$(shell readlink -f scripts/launch_qemu.sh)
RUNNER_TEST=$(shell readlink -f scripts/test_runner.sh)

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=net1,hostfwd=tcp::18080-:18080 \
		-device rtl8139,netdev=net1 \
		-object filter-dump,id=f2,netdev=net1,file=log/dump_net1.pcap \
		-m 1024M \
		-drive format=raw,file=fat:rw:mnt \
		-chardev file,id=char_com1,mux=on,path=log/com1.txt \
		-chardev stdio,id=char_com2,mux=on,logfile=log/com2.txt \
		-serial chardev:char_com1 \
		-serial chardev:char_com2 \
		-rtc base=localtime \
		-monitor telnet:0.0.0.0:$(PORT_MONITOR),server,nowait \
		--no-reboot \
		-d int,cpu_reset \
		-D log/qemu_debug.txt \
		-device usb-tablet \
		-device usb-kbd \
		${MORE_QEMU_FLAGS}

ifndef DISPLAY
QEMU_ARGS+=-vnc 0.0.0.0:$(PORT_OFFSET_VNC),password=on
endif

#		-vnc 0.0.0.0:$(PORT_OFFSET_VNC),password=on \
#		-gdb tcp::3333 -S \
# -device usb-host,hostbus=1,hostport=1 \
# -device usb-host,hostbus=2,hostport=1.2.1 \
# -device usb-mouse \
# -netdev user,id=net0 -device usb-net,netdev=net0 \
# -object filter-dump,id=f1,netdev=net0,file=log/dump_net0.dat \

HOST_TARGET=`rustc -V -v | grep 'host:' | sed 's/host: //'`
CLIPPY_OPTIONS=-D warnings

default: bin

.PHONY: \
	app \
	commit \
	spellcheck \
	run \
	run_deps \
	watch_serial \
	# Keep this line blank

.PHONY : bin
bin: generated/font.rs
	rustup component add rust-src
	cd os && cargo build --all-targets --all-features

.PHONY : clippy
clippy:
	rustup component add clippy
	cd font && cargo clippy -- ${CLIPPY_OPTIONS}
	cd os && cargo clippy -- ${CLIPPY_OPTIONS}
	cd dbgutil && cargo clippy
	# cd font && cargo clippy --all-features --all-targets -- -D warnings
	cd os && cargo clippy --all-features --all-targets -- -D warnings

.PHONY : dump_config
dump_config:
	@echo "Host target: $(HOST_TARGET)"

.PHONY : test
test:
	make internal_run_app_test INIT="hello1"
	make run_os_test
	make run_os_lib_test
	make run_e2e_test

.PHONY : fmt
fmt :
	rustup component add rustfmt
	cargo fmt

.PHONY : commit
commit :
	make fmt
	git add .
	make filecheck
	make rustcheck
	make clippy
	make spellcheck
	make # build
	make test
	./scripts/ensure_objs_are_not_under_git_control.sh
	git add .
	git diff HEAD --color=always | less -R
	git commit

generated/font.rs: font/font.txt Makefile
	mkdir -p generated
	cargo run --bin font font/font.txt > $@.tmp
	# Rename the file once the command above is succeeded to avoid issues.
	mv $@.tmp $@

.PHONY : filecheck
filecheck:
	@! git ls-files | grep -v -E '(\.(rs|md|toml|sh|txt|json|lock|mk)|Makefile|LICENSE|rust-toolchain|Dockerfile|OVMF.fd|\.yml|\.gitignore|\.gitkeep)$$' \
		|| ! echo "!!! Unknown file type is being added! Do you really want to commit the file above? (if so, just modify filecheck recipe)"

.PHONY : spellcheck
spellcheck :
	@scripts/spellcheck.sh recieve receive
	@scripts/spellcheck.sh faild failed
	@scripts/spellcheck.sh mappng mapping

.PHONY : rustcheck
rustcheck :
	@scripts/rustcheck.sh

.PHONY : run
run :
	export INIT="${INIT}" && \
		cd os && cargo \
		  --config "target.'cfg(target_os = \"uefi\")'.runner = '$(RUNNER_NORMAL)'" \
		run --release

.PHONY : run_e2e_test
run_e2e_test :
	cd e2etest && cargo test

.PHONY : run_os_test
run_os_test :
	export INIT="${INIT}" && \
		cd os && cargo \
		  --config "target.'cfg(target_os = \"uefi\")'.runner = '$(RUNNER_TEST)'" \
		test --bin os

.PHONY : run_os_lib_test
run_os_lib_test :
	export INIT="${INIT}" && \
		cd os && cargo \
		  --config "target.'cfg(target_os = \"uefi\")'.runner = '$(RUNNER_TEST)'" \
		test --lib

.PHONY : app
app :
	-rm -r generated/bin
	make -C app/hello0
	make -C app/hello1
	make -C app/uname
	make -C app/loop
	make -C app/window0

.PHONY : run_deps
run_deps : app
	-rm -rf mnt
	mkdir -p mnt/
	mkdir -p mnt/EFI/BOOT
	cp README.md mnt/
	cp generated/bin/* mnt/

.PHONY : watch_serial
watch_serial:
	while ! telnet localhost 1235 ; do sleep 1 ; done ;

.PHONY : watch_qemu_monitor
watch_qemu_monitor:
	while ! telnet localhost ${PORT_MONITOR} ; do sleep 1 ; done ;

generated/bin/os.efi:
	cd os && cargo install --path . --root ../generated/

.PHONY : install
install : run_deps generated/bin/os.efi
	bash ./scripts/install.sh

.PHONY : internal_launch_qemu
internal_launch_qemu : run_deps
	@echo "Using ${PATH_TO_EFI}"
	cp ${PATH_TO_EFI} mnt/EFI/BOOT/BOOTX64.EFI
	$(QEMU) $(QEMU_ARGS)

.PHONY : internal_run_app_test
internal_run_app_test : run_deps generated/bin/os.efi
	cp  generated/bin/os.efi mnt/EFI/BOOT/BOOTX64.EFI
	@echo Testing app $$INIT...
	printf "$$INIT" > mnt/init.txt
	$(QEMU) $(QEMU_ARGS) -display none ; \
		RETCODE=$$? ; \
		if [ $$RETCODE -eq 3 ]; then \
			echo "\nPASS" ; \
			exit 0 ; \
		else \
			echo "\nFAIL: QEMU returned $$RETCODE" ; \
			exit 1 ; \
		fi

.PHONY : internal_run_os_test
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

.PHONY : act
act:
	act -P hikalium/wasabi-builder:latest=hikalium/wasabi-builder:latest

.PHONY : symbols
symbols:
	cargo run --bin=dbgutil -- symbol | tee symbols.txt

.PHONY : objdump
objdump:
	cargo install cargo-binutils
	rustup component add llvm-tools-preview
	cd os && cargo-objdump -- -d > ../objdump.txt
	echo "Saved objdump as objdump.txt"

.PHONY : crash
crash:
	cargo run --bin=dbgutil crash \
		--qemu-debug-log $$(readlink -f log/qemu_debug.txt) \
		--serial-log $$(readlink -f log/com2.txt)

generated/noVNC-% :
	wget -O generated/novnc.tar.gz https://github.com/novnc/noVNC/archive/refs/tags/v$*.tar.gz
	cd generated && tar -xvf novnc.tar.gz

NOVNC_VERSION=1.4.0
.PHONY : vnc
vnc: generated/noVNC-$(NOVNC_VERSION)
	( echo 'change vnc password $(VNC_PASSWORD)' | while ! nc localhost $(PORT_MONITOR) ; do sleep 1 ; done ) &
	generated/noVNC-$(NOVNC_VERSION)/utils/novnc_proxy --vnc localhost:$$((5900+${PORT_OFFSET_VNC}))
