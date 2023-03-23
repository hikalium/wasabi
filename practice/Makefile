RUSTC=$(shell rustup which rustc)
TARGETS=\
		minimal_linux_elf_0 \
		minimal_linux_elf_1 \
		minimal_linux_elf_2 \
		minimal_linux_elf_3 \
		minimal_linux_elf_4 \
		minimal_linux_elf_5 \
		minimal_linux_elf_6 \
		minimal_linux_elf_7 \
# keep this line

.PHONY : default
default : $(addsuffix .bin, $(TARGETS))

.PHONY : run
run : $(addsuffix .run, $(TARGETS))

.PHONY : %.run
%.run : %.bin
	@file $*.bin
	@stat -c "%n,%s" $*.bin | column -t -s,
	readelf -h $*.bin  | grep -e Type -e Entry
	./$*.bin ; echo $$?
	@echo ""
	@echo ""

.PHONY : clean
clean :
	-rm *.bin

# https://doc.rust-lang.org/cargo/reference/profiles.html

minimal_linux_elf_0.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs 

minimal_linux_elf_1.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=debuginfo 

minimal_linux_elf_2.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols 

minimal_linux_elf_3.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" 

minimal_linux_elf_4.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 

minimal_linux_elf_5.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort 

minimal_linux_elf_6.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true 

minimal_linux_elf_7.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -Clink-args=-nostartfiles
