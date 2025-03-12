#!/bin/bash -e

function do_install() {
    pushd "$1"
    pwd
    mkdir -p ./EFI/BOOT/
    cp "${EFI_PATH}" ./EFI/BOOT/BOOTX64.EFI
    find ./EFI
    popd > /dev/null
}

cd "$(dirname "$(cargo locate-project --message-format plain)")"
EFI_PATH="$1"
EFI_PATH="$(readlink -f ${EFI_PATH})"
echo "Using image at: ${EFI_PATH}"
ls -lah ${EFI_PATH}
file "${EFI_PATH}"

if cat /opt/google/cros-containers/etc/lsb-release | grep 'Chrome OS' ; then
    # For crostini (Linux environment on ChromeOS)
    if ls -lahd /mnt/chromeos/removable/WASABIOS ; then
        do_install /mnt/chromeos/removable/WASABIOS
        echo 'Done!'
        echo 'Please unmount from the File app first then remove the disk!'
    else
        echo 'Disk "WASABIOS" not found under /mnt/chromeos/removable/.'
        echo 'Please insert a disk and share it with Linux from the File app.'
        exit 1
    fi
else
    # For bare-metal Linux environment
    DISK=`readlink -f /dev/disk/by-partlabel/WASABIOS`
    echo "Write WasabiOS to ${DISK}. Are you sure?"
    read -p "[Enter to proceed, or Ctrl-C to abort] " REPLY
    mkdir -p ./usb
    sudo mount ${DISK} ./usb
    do_install ./usb
    sudo umount ${DISK}
fi
