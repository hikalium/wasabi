#!/bin/bash -e
cd `dirname $0`
cd ..
NG_SPELL=$1
OK_SPELL=$2
set +e
RESULT=`git grep -i ${NG_SPELL} | grep -v 'scripts/spellcheck.sh'`
RESULT_STATUS=$?
set -e
if test $RESULT_STATUS -eq 0; then
	echo "$RESULT"
	echo "FAIL: NG patterns found: Please replace '${NG_SPELL}' to '${OK_SPELL}'"
	exit 1
elif test $RESULT_STATUS -eq 1; then
	echo "PASS: No matches found: NG: '${NG_SPELL}' => OK: '${OK_SPELL}'"
	exit 0
else
	echo "FAIL: something went wrong"
	exit 1
fi
