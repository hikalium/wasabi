RUSTC=$(shell rustup which rustc)
TARGETS=\
		minimal_linux_elf_0 \
		minimal_linux_elf_1 \
		minimal_linux_elf_2 \
# keep this line

.PHONY : default
default : $(addsuffix .bin, $(TARGETS))

.PHONY : run
run : $(addsuffix .run, $(TARGETS))

.PHONY : %.run
%.run : %.bin
	@echo ""
	@echo ""
	file $*.bin
	ls -lah $*.bin
	readelf -h $*.bin  | grep -e Type -e Entry
	./$*.bin ; echo $$?

# https://doc.rust-lang.org/cargo/reference/profiles.html

minimal_linux_elf_0.bin : minimal_linux_elf_0.rs Makefile
	$(RUSTC) -o $@ minimal_linux_elf_0.rs

minimal_linux_elf_1.bin : minimal_linux_elf_1.rs Makefile
	$(RUSTC) -O -C strip=debuginfo -o $@ minimal_linux_elf_1.rs

minimal_linux_elf_2.bin : minimal_linux_elf_2.rs Makefile
	$(RUSTC) -O -C strip=symbols -o $@ minimal_linux_elf_2.rs
