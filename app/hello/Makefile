bulid:
	cargo build
	cargo install --path . --root ../../generated/
	file ../../generated/bin/hello
	ls -lah ../../generated/bin/hello
	strip ../../generated/bin/hello
	ls -lah ../../generated/bin/hello
	objdump -d ../../generated/bin/hello
	hexdump -C ../../generated/bin/hello
	readelf -S ../../generated/bin/hello
	readelf -l ../../generated/bin/hello
