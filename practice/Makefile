RUSTC=$(shell rustup which rustc)
TARGETS=\
		minimal_linux_elf_00 \
		minimal_linux_elf_01 \
		minimal_linux_elf_02 \
		minimal_linux_elf_03 \
		minimal_linux_elf_04 \
		minimal_linux_elf_05 \
		minimal_linux_elf_06 \
		minimal_linux_elf_07 \
		minimal_linux_elf_08 \
		minimal_linux_elf_09 \
		minimal_linux_elf_10 \
		minimal_linux_elf_11 \
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
	readelf -l $*.bin
	./$*.bin ; echo $$?
	@echo ""
	@echo ""

.PHONY : clean
clean :
	-rm *.bin

# https://doc.rust-lang.org/cargo/reference/profiles.html

minimal_linux_elf_00.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs 

minimal_linux_elf_01.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=debuginfo 

minimal_linux_elf_02.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols 

minimal_linux_elf_03.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" 

minimal_linux_elf_04.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 

minimal_linux_elf_05.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort 

minimal_linux_elf_06.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_base.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true 

minimal_linux_elf_07.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles
	
# ASLR worked
minimal_linux_elf_08.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles -C target-cpu=native

.PHONY : minimal_linux_elf_09.bin
minimal_linux_elf_09.bin :
	# ASLR didn't work
	#$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles -C target-cpu=native -Clink-args=-Wl,-n,-N,--no-dynamic-linker
	# -n and -N can not be appended, since they will make the binary not pie (why...?)
	# ASLR worked!
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles -C target-cpu=native -Clink-args=-Wl,--hash-style=sysv,--no-dynamic-linker,--build-id=none,--no-eh-frame-hdr,-z,norelro,-z,execstack
	objcopy --strip-unneeded -R .hash -R .eh_frame -R .dynstr -R .dynsym minimal_linux_elf_09.bin

minimal_linux_elf_10.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles -C target-cpu=native -Clink-args=-Wl,-n,-N,--no-dynamic-linker,--build-id=none,--no-eh-frame-hdr
	objcopy --dump-section .dynamic=minimal_linux_elf_10.section.dynamic.bin minimal_linux_elf_10.bin
	objcopy --dump-section .dynsym=minimal_linux_elf_10.section.dynsym.bin minimal_linux_elf_10.bin

minimal_linux_elf_11.bin : 
	$(RUSTC) -o $@ minimal_linux_elf_nostd.rs -O -C strip=symbols -C opt-level="z" -C codegen-units=1 -C panic=abort -C lto=true -C link-args=-nostartfiles -C target-cpu=native -Clink-args=-Wl,-n,-N,--no-dynamic-linker,--build-id=none,--no-eh-frame-hdr,--no-pie
