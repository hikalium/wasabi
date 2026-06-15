#!/bin/bash -e
PROJ_ROOT="$(dirname $(dirname ${BASH_SOURCE:-$0}))"
cd "${PROJ_ROOT}"

PATH_TO_EFI="$1"
rm -rf mnt
mkdir -p mnt/EFI/BOOT/
cp ${PATH_TO_EFI} mnt/EFI/BOOT/BOOTX64.EFI
set +e
mkdir -p log

# Prefer the patched QEMU (with usb-ncm) bundled in the wasabi_devtools
# sibling directory if it is present; otherwise fall back to the
# qemu-system-x86_64 found on PATH.
QEMU=qemu-system-x86_64
BUNDLED_QEMU="../wasabi_devtools/qemu/bin/qemu-system-x86_64"
if [ -x "${BUNDLED_QEMU}" ]; then
  QEMU="${BUNDLED_QEMU}"
  echo "Using bundled QEMU: ${BUNDLED_QEMU}"
fi

"${QEMU}" \
  -m 4G \
  -bios third_party/ovmf/RELEASEX64_OVMF.fd \
  -machine q35 \
  -drive format=raw,file=fat:rw:mnt \
  -monitor telnet:0.0.0.0:2345,server,nowait,logfile=log/qemu_monitor.txt \
  -chardev stdio,id=char_com1,mux=on,logfile=log/com1.txt \
  -serial chardev:char_com1 \
  -device qemu-xhci \
  -device usb-kbd \
  -device usb-tablet \
  -device isa-debug-exit,iobase=0xf4,iosize=0x01
RETCODE=$?
set -e
if [ $RETCODE -eq 0 ]; then
  exit 0
elif [ $RETCODE -eq 3 ]; then
  printf "\nPASS\n"
  exit 0
else
  printf "\nFAIL: QEMU returned $RETCODE\n"
  exit 1
fi
