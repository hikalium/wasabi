#!/bin/bash -e
cd `dirname $0`
cd ..
set +e
RESULT=`git grep '::' *.rs | grep -v -E ':\s*use' | grep -E '::[a-zA-Z0-9_]+::[a-zA-Z0-9_]+::'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "${RESULT}"
	echo "FAIL: Please add 'use' for long nested names listed above:"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: rustcheck"
	exit 0
else
	echo "FAIL: something went wrong"
	exit 1
fi
