NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
BINPATH=$(ROOT)/bin/
TARGET=$(shell readlink -f ../../target/x86_64-unknown-none/debug/hello1)

.PHONY : .FORCE

$(TARGET): .FORCE
	rustup target add x86_64-unknown-none
	RUSTFLAGS="-C strip=symbols -C link-arg=-eentry -C no-redzone=true" cargo build --target x86_64-unknown-none -vvv

.PHONY : install
install: $(TARGET)
	cp $(TARGET) $(BINPATH)

.PHONY : objdump
objdump: $(TARGET)
	objdump -d $(TARGET)

.PHONY : run
run:
	make -C ../../ run
