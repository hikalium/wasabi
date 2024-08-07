PROJECT_ROOT:=$(realpath $(dir $(abspath $(lastword $(MAKEFILE_LIST)))))

OVMF=${PROJECT_ROOT}/third_party/ovmf/RELEASEX64_OVMF.fd
QEMU=qemu-system-x86_64
PORT_MONITOR?=2345
PORT_OFFSET_VNC?=5
VNC_PASSWORD?=wasabi
INIT?=hello1
RUNNER_NORMAL=$(shell readlink -f scripts/launch_qemu.sh)
RUNNER_TEST=$(shell readlink -f scripts/test_runner.sh)
NOVNC_VERSION=1.4.0
HOST_TARGET=`rustc -V -v | grep 'host:' | sed 's/host: //'`
CLIPPY_OPTIONS=-D warnings
APPS=$(shell ls $(PROJECT_ROOT)/app)
APP_PACKAGES=$(addprefix  -p , $(APPS))
BIN_DIR=$(PROJECT_ROOT)/generated/bin/
TCP_FORWARD_PORT?=18080

QEMU_ARGS=\
		-machine q35 -cpu qemu64 -smp 4 \
		-bios $(OVMF) \
		-device qemu-xhci \
		-device isa-debug-exit,iobase=0xf4,iosize=0x01 \
		-netdev user,id=net1,hostfwd=tcp::$(TCP_FORWARD_PORT)-:$(TCP_FORWARD_PORT) \
		-device rtl8139,netdev=net1 \
		-object filter-dump,id=f2,netdev=net1,file=log/dump_net1.pcap \
		-m 1024M \
		-drive format=raw,file=fat:rw:mnt \
		-chardev file,id=char_com1,mux=on,path=log/com1.txt \
		-chardev stdio,id=char_com2,mux=on,logfile=log/com2.txt \
		-serial chardev:char_com1 \
		-serial chardev:char_com2 \
		-rtc base=localtime \
		-monitor telnet:0.0.0.0:$(PORT_MONITOR),server,nowait,logfile=log/qemu_monitor.txt \
		--no-reboot \
		-d int,cpu_reset \
		-D log/qemu_debug.txt \
		-device usb-kbd \
		-device usb-tablet,usb_version=1 \
		${MORE_QEMU_FLAGS}

# > -device usb-tablet,usb_version=1
# setting usb_version=1 here to avoid issues.
# c.f. https://bugzilla.redhat.com/show_bug.cgi?id=929068#c2

ifndef DISPLAY
QEMU_ARGS+= -vnc 0.0.0.0:$(PORT_OFFSET_VNC),password=on
endif
ifeq ($(HEADLESS),1)
QEMU_ARGS+= -display none
endif

# QEMU_ARGS+= -device usb-tablet
#		-vnc 0.0.0.0:$(PORT_OFFSET_VNC),password=on \
#		-gdb tcp::3333 -S \
# -device usb-host,hostbus=1,hostport=1 \
# -device usb-host,hostbus=2,hostport=1.2.1 \
# -device usb-mouse \
# -netdev user,id=net0 -device usb-net,netdev=net0 \
# -object filter-dump,id=f1,netdev=net0,file=log/dump_net0.dat \

APP_RUSTFLAGS=\
		  -C link-args=-e \
		  -C link-args=entry \
		  -C link-args=-z \
		  -C link-args=execstack
APP_TARGET=x86_64-unknown-none
APP_CARGO=RUSTFLAGS='$(APP_RUSTFLAGS)' cargo
APP_BUILD_ARG=-v --target $(APP_TARGET) --release

.PHONY : default
default: os

.PHONY : os
os:
	rustup component add rust-src --toolchain `rustup show active-toolchain | cut -d ' ' -f 1`
	cd os && cargo build --all-targets --all-features

.PHONY : clippy
clippy:
	rustup component add clippy
	cd font && cargo clippy -- ${CLIPPY_OPTIONS}
	cd os && cargo clippy -- ${CLIPPY_OPTIONS}
	cd noli && cargo clippy -- ${CLIPPY_OPTIONS}
	cd e2etest && cargo clippy -- ${CLIPPY_OPTIONS}
	cd dbgutil && cargo clippy -- ${CLIPPY_OPTIONS}
	cd os && cargo clippy --all-features --all-targets -- ${CLIPPY_OPTIONS}
	# Run clippy on all apps under app dir
	cargo clippy $(APP_PACKAGES) -- ${CLIPPY_OPTIONS}
	cargo clippy --target=x86_64-unknown-none $(APP_PACKAGES) -- ${CLIPPY_OPTIONS}

.PHONY : dump_config
dump_config:
	@echo "Host target: $(HOST_TARGET)"

.PHONY : test
test :
	make pre_upload_test

.PHONY : check
check:
	make filecheck
	./scripts/ensure_objs_are_not_under_git_control.sh
	make rustcheck
	make clippy
	make spellcheck
	make os

.PHONY : run_app_unit_test
run_app_unit_test:
	cargo test $(APP_PACKAGES)

.PHONY : pre_upload_test
pre_upload_test:
	cargo test --package noli
	make run_os_test
	make run_os_lib_test
	make run_app_unit_test

.PHONY : post_upload_test
post_upload_test:
	make run_e2e_test

.PHONY : fmt
fmt :
	rustup component add rustfmt
	cargo fmt

.PHONY : commit
commit :
	make fmt
	git add .
	make check
	make pre_upload_test
	git diff HEAD --color=always | less -R
	git commit

.PHONY : filecheck
filecheck:
	@! git ls-files | grep -v -E '(\.(rs|md|toml|sh|txt|json|lock|mk)|Makefile|LICENSE|rust-toolchain|Dockerfile|OVMF.fd|\.yml|\.gitignore|\.gitkeep)$$' \
		|| ! echo "!!! Unknown file type is being added! Do you really want to commit the file above? (if so, just modify filecheck recipe)"

.PHONY : spellcheck
spellcheck :
	@scripts/spellcheck.sh recieve receive
	@scripts/spellcheck.sh faild failed
	@scripts/spellcheck.sh mappng mapping
	@scripts/spellcheck.sh priviledged privileged

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
	@echo ">>>>>>>> Running e2etest with FILTER=\"$(FILTER)\""
	cd e2etest && cargo test -- --test-threads=1 --nocapture $(FILTER)

.PHONY : run_os_test
run_os_test :
	cd os && cargo \
		  --config "target.'cfg(target_os = \"uefi\")'.runner = '$(RUNNER_TEST)'" \
		test --bin os

.PHONY : run_os_lib_test
run_os_lib_test :
	cd os && cargo \
		  --config "target.'cfg(target_os = \"uefi\")'.runner = '$(RUNNER_TEST)'" \
		test --lib

.PHONY : app
app :
	rustup target add $(APP_TARGET)
	[ -d $(BIN_DIR) ] && rm -rf $(BIN_DIR) ; true
	$(APP_CARGO) build $(APP_BUILD_ARG) $(APP_PACKAGES)
	mkdir -p $(BIN_DIR)
	@echo "Installing apps to ${BIN_DIR}"
	cd target/x86_64-unknown-none/release && cp $(APPS) $(BIN_DIR)

.PHONY : run_deps
run_deps : app
	-rm -rf mnt
	mkdir -p mnt/
	mkdir -p mnt/EFI/BOOT
	cp README.md mnt/
	cp generated/bin/* mnt/
	cp default/init.txt mnt/

.PHONY : watch_serial
watch_serial:
	while ! telnet localhost 1235 ; do sleep 1 ; done ;

.PHONY : watch_qemu_monitor
watch_qemu_monitor:
	while ! telnet localhost ${PORT_MONITOR} ; do sleep 1 ; done ;

.PHONY : install
install : run_deps generated/bin/os.efi
	bash ./scripts/install.sh

.PHONY : internal_launch_qemu
internal_launch_qemu : run_deps
	@echo "Using ${PATH_TO_EFI}"
	cp ${PATH_TO_EFI} mnt/EFI/BOOT/BOOTX64.EFI
	$(QEMU) $(QEMU_ARGS) 2>log/qemu_stderr.txt

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

.PHONY : tshark
tshark:
	@tshark -V -o ip.check_checksum:TRUE -o tcp.check_checksum:TRUE -r log/dump_net1.pcap

.PHONY : tshark_json
tshark_json:
	@tshark -V -o ip.check_checksum:TRUE -x -T json -r log/dump_net1.pcap | jq .

.PHONY : tshark_tcp
tshark_tcp:
	@tshark -V -o ip.check_checksum:TRUE -T json -Y tcp -r log/dump_net1.pcap | jq -c '.[]._source.layers | {src_ip: .ip."ip.src", dst_ip: .ip."ip.dst", src: .tcp."tcp.srcport", dst: .tcp."tcp.dstport", flags: .tcp."tcp.flags_tree"."tcp.flags.str", seq: .tcp."tcp.seq_raw", ack_raw: .tcp."tcp.ack_raw"}'

.PHONY : tcp_hello
tcp_hello:
	echo "hello" | nc localhost ${TCP_FORWARD_PORT}

.PHONY : vnc
vnc: generated/noVNC-$(NOVNC_VERSION)
	( echo 'change vnc password $(VNC_PASSWORD)' | while ! nc localhost $(PORT_MONITOR) ; do sleep 1 ; done ) &
	generated/noVNC-$(NOVNC_VERSION)/utils/novnc_proxy --vnc localhost:$$((5900+${PORT_OFFSET_VNC}))

generated/bin/os.efi:
	cd os && cargo install --path . --root ../generated/

generated/noVNC-% :
	wget -O generated/novnc.tar.gz https://github.com/novnc/noVNC/archive/refs/tags/v$*.tar.gz
	cd generated && tar -xvf novnc.tar.gz

list_structs:
	git grep -o 'struct [A-Z][A-Za-z0-9]* ' | sed -e 's#/src/#::#' -e 's#main\.rs:##' -e 's#lib\.rs:##' -e 's#struct ##' -e 's/\.rs:/::/' -e 's#/#::#' | sort -u
