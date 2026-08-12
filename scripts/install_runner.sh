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

if cat /opt/google/cros-containers/etc/lsb-release 2> /dev/null | grep 'Chrome OS' ; then
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
    # The disk can be labeled either as a GPT partition or as a filesystem
    DISK=''
    for LABEL_PATH in /dev/disk/by-partlabel/WASABIOS /dev/disk/by-label/WASABIOS ; do
        if [ -e "${LABEL_PATH}" ] ; then
            DISK="$(readlink -f "${LABEL_PATH}")"
            break
        fi
    done
    if [ -z "${DISK}" ] ; then
        echo 'Disk "WASABIOS" not found under /dev/disk/by-partlabel/ nor /dev/disk/by-label/.'
        echo 'Please insert a disk which has a FAT partition labeled as "WASABIOS".'
        exit 1
    fi
    echo "Write WasabiOS to ${DISK}. Are you sure?"
    read -p "[Enter to proceed, or Ctrl-C to abort] " REPLY
    # Desktop environments tend to mount the disk automatically
    MOUNT_POINT="$(findmnt -n -f -o TARGET --source "${DISK}" || true)"
    if [ -n "${MOUNT_POINT}" ] ; then
        do_install "${MOUNT_POINT}"
        sync
    else
        mkdir -p ./usb
        sudo mount "${DISK}" ./usb
        do_install ./usb
        sudo umount "${DISK}"
    fi
fi
