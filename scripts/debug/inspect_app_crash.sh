#!/bin/bash -e
RET_TO_IN_MEMORY=`cat qemu_debug.log | grep hikalium | grep ret_to | cut -d ':' -f 3`
ENTRY_POINT_IN_MEMORY=`cat com2.log | grep -a -e entry_point | cut -d '=' -f 2`
ENTRY_POINT_IN_OBJDUMP=0x`make objdump_hello | grep '<entry>:' | cut -d ' ' -f 1`
RET_TO_IN_OBJDUMP=`printf "0x%X\n" $((${RET_TO_IN_MEMORY} - ${ENTRY_POINT_IN_MEMORY} + ${ENTRY_POINT_IN_OBJDUMP}))`
LINE_RET_TO="$(make objdump_hello | grep "`echo ${RET_TO_IN_OBJDUMP} | sed -E 's/^0x//' | tr '[:upper:]' '[:lower:]'`")"
ADDR_RET_TO="`echo ${LINE_RET_TO} | cut -d ' ' -f 1`"
FN_SYMBOL=`make objdump_hello | grep -E -e ">:$" -e "${ADDR_RET_TO}" | grep -B 1 -e "${ADDR_RET_TO}" | head -n 1`

echo ${FN_SYMBOL}
make objdump_hello | grep -B 1 -e "${ADDR_RET_TO}"
