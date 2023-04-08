TARGET=x86_64-unknown-none
NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
RUSTFLAGS=\
		  -C no-redzone=true \
		  -C panic=abort \
		  -C link-args=-eentry,-Wl,--hash-style=sysv,--no-dynamic-linker,--build-id=none,--no-eh-frame-hdr,-z,norelro,-z,execstack
CARGO=RUSTFLAGS='${RUSTFLAGS}' $(shell which cargo)

.PHONY : build
build :
	rustup target add $(TARGET)
	$(CARGO) build --target $(TARGET) -vvv

.PHONY : install
install :
	rustup target add $(TARGET)
	$(CARGO) install --force --target $(TARGET) --root $(ROOT) --path .

.PHONY : objdump
objdump :
	cargo install cargo-binutils
	rustup component add llvm-tools-preview
	$(CARGO) objdump --bin $(NAME) -- -d

.PHONY : run
run :
	make -C ../../ run
