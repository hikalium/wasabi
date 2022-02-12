#!/bin/bash -xe
LOADER_TEST_EFI="$1"
cd `dirname $0`
cd ..
LOADER_TEST_EFI="${LOADER_TEST_EFI}" make internal_run_os_test
