TARGET=x86_64-unknown-none
NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
CARGO=cargo
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
