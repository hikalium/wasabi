TARGET=x86_64-unknown-none
NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
RUSTFLAGS=\
		  -C no-redzone=true \
		  -C panic=abort \
		  -C link-args=-eentry,-Wl,--hash-style=sysv,--no-dynamic-linker,--build-id=none,--no-eh-frame-hdr,-z,norelro,-z,execstack

.PHONY : build
build :
	rustup target add $(TARGET)
	RUSTFLAGS="${RUSTFLAGS}" \
			  cargo build --target $(TARGET) -vvv

.PHONY : install
install :
	rustup target add $(TARGET)
	RUSTFLAGS="${RUSTFLAGS}" \
		cargo install --force --target $(TARGET) --root $(ROOT) --path .

.PHONY : objdump
objdump : $(NAME).bin
	cargo install cargo-binutils
	cargo objdump --bin $(NAME) -- -d

.PHONY : run
run :
	make -C ../../ run
