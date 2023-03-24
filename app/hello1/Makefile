NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
BINPATH=$(ROOT)/bin/
TARGET=$(shell readlink -f ../../target/x86_64-unknown-none/debug/hello1)

.PHONY : .FORCE

$(TARGET): .FORCE
	cargo build --target x86_64-unknown-none -vvv

install: $(TARGET)
	cp $(TARGET) $(BINPATH)
