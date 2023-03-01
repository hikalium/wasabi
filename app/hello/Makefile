ROOT=$(shell readlink -f ../../generated)

.PHONY : bulid
build:
	cargo install --path . --root $(ROOT)
	file ../../generated/bin/hello
	ls -lah ../../generated/bin/hello

.PHONY : dump
dump: build
	objdump -d $(ROOT)/bin/hello


.PHONY : run
run:
	make -C ../../ run
