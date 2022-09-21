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

echo "PASS: rustcheck done"
exit 0
