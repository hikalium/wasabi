NAME=$(shell cargo read-manifest | jq -r .name)
ROOT=$(shell readlink -f ../../generated)
BINPATH=$(ROOT)/bin/$(NAME)

.PHONY : bulid
build:
	cargo install --path . --root $(ROOT)

.PHONY : objdump
objdump: build
	objdump -d $(BINPATH)

.PHONY : file
file: build
	file $(BINPATH)

.PHONY : ls
ls: build
	ls -lah $(BINPATH)

.PHONY : run
run:
	make -C ../../ run
