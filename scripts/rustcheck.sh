#!/bin/bash -e
cd `dirname $0`
cd ..

set +e
RESULT=`git grep -n '::' *.rs | grep -v -E ':\s*use' | grep -E '::[a-zA-Z0-9_]+::[a-zA-Z0-9_]+::'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${RESULT}"
	echo "----"
	echo "FAIL: Please add 'use' for long nested names listed above:"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No too long name space usage found"
else
	echo "FAIL: something went wrong"
	exit 1
fi

set +e
RESULT=`git grep -n -E '^\s*use' *.rs | grep '{'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${RESULT}"
	echo "----"
	echo "FAIL: Please ungroup 'use' declarations listed above:"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No grouped 'use' declarations found"
else
	echo "FAIL: something went wrong"
	exit 1
fi

set +e
RESULT=`git grep -E '^use' | grep '\*' | grep -v -E '^[^:]+:use .*::prelude::\*;$'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${RESULT}"
	echo "----"
	echo "FAIL: Please remove glob (wildcard) 'use' listed above:"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No glob (wildcard) 'use' usage found (except for noli prelude)"
else
	echo "FAIL: something went wrong"
	exit 1
fi

set +e
FILES=`git ls-files *.rs`
LIST=`wc -l ${FILES} | sort -n | grep '.rs$' | awk '$1>999' | grep '.rs$'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${LIST}"
	echo "----"
	echo "FAIL: Please reduce the number of lines in the files listed above"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No toooooo long files were found"
else
	echo "FAIL: something went wrong"
	exit 1
fi

set +e
LIST=`git grep 'test *= *false' | grep toml:`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${LIST}"
	echo "----"
	echo "FAIL: Please don't disable tests for crates. You can make it work...!"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No disabled tests at crate level found"
else
	echo "FAIL: something went wrong"
	exit 1
fi

set +e
LIST=`git grep 'asm!' | cut -d ':' -f 1 | uniq | grep '^os/src/' | grep -v '^os/src/x86_64'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "----"
	echo "${LIST}"
	echo "----"
	echo "FAIL: asm! macro is prohibited outside of arch-specific implementation. Please remove them."
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No asm! macro usage found in arch-independent part"
else
	echo "FAIL: something went wrong"
	exit 1
fi


echo "PASS: rustcheck done"
exit 0
