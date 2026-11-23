#!/bin/bash
# A canned HTTP server for tests/http_get.rs. QEMU runs this script once
# per TCP connection the guest opens to 10.0.2.100:80 (see the guestfwd
# option in launch_qemu.sh), with the connection on stdin and stdout.
# It answers "GET /hello" for the right host with 200 and anything else
# with 404, then exits, which closes the connection.
read -r request_line
host_line=""
while read -r line; do
  line="${line%$'\r'}"
  [ -z "${line}" ] && break
  case "${line}" in
    Host:*) host_line="${line}" ;;
  esac
done
if [ "${request_line%$'\r'}" = "GET /hello HTTP/1.1" ] && \
   [ "${host_line}" = "Host: 10.0.2.100" ]; then
  printf 'HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nhello from the test server\n'
else
  printf 'HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\n\r\nnot found\n'
fi
