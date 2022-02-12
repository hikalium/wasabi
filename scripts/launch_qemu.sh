#!/bin/bash -xe
PATH_TO_EFI="$1"
cd `dirname $0`
cd ..
PATH_TO_EFI="${PATH_TO_EFI}" make internal_launch_qemu
