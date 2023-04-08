TARGET=x86_64-unknown-none
NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
RUSTFLAGS=\
		  --target $(TARGET) \
		  -C no-redzone=true \
		  -C panic=abort \
		  -C link-args=-e \
		  -C link-args=entry \
		  -C link-args=--static \
		  -C link-args=--hash-style=sysv \
		  -C link-args=--no-dynamic-linker \
		  -C link-args=--build-id=none \
		  -C link-args=--no-eh-frame-hdr \
		  -C link-args=-z \
		  -C link-args=norelro \
		  -C link-args=-z \
		  -C link-args=execstack
CARGO=RUSTFLAGS='${RUSTFLAGS}' cargo
BIN_PATH_DEBUG=$(shell cargo metadata --format-version 1 | jq -r .target_directory)/debug/$(NAME)

.PHONY : build
build :
	rustup target add $(TARGET)
	$(CARGO) build

.PHONY : install
install :
	rustup target add $(TARGET)
	$(CARGO) install --force --root $(ROOT) --path .

.PHONY : objdump
objdump :
	cargo install cargo-binutils
	rustup component add llvm-tools-preview
	$(CARGO) objdump -- -d

.PHONY : nm
nm :
	cargo install cargo-binutils
	rustup component add llvm-tools-preview
	$(CARGO) nm

.PHONY : readelf
readelf : build
	readelf -l $(BIN_PATH_DEBUG)

.PHONY : hexdump
hexdump : build
	hexdump -C $(BIN_PATH_DEBUG)

.PHONY : run
run :
	make -C ../../ run
