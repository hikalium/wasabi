#!/bin/bash -e
mkdir -p mnt/EFI/BOOT/
cp generated/bin/os.efi mnt/EFI/BOOT/BOOTX64.EFI
TMPDIR=`mktemp -d`
DISK=`readlink -f /dev/disk/by-partlabel/WASABIOS`
if cat /opt/google/cros-containers/etc/lsb-release | grep 'Chrome OS' ; then
	if ls -lahd /mnt/chromeos/removable/WASABIOS ; then
		cp -r mnt/* /mnt/chromeos/removable/WASABIOS/
		echo 'Done! Please remove the disk!'
	else
		echo 'Disk "WASABIOS" not found. Please insert the disk and share it with Linux from the File app.'
		exit 1
	fi
else
	read -p "Write WasabiOS to ${DISK}. Are you sure? [Enter to proceed, or Ctrl-C to abort] " REPLY
	sudo mount ${DISK} ${TMPDIR}
	sudo cp -r mnt/* ${TMPDIR}/
	sudo umount ${DISK}
fi
