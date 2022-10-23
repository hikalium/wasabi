```
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1 | cargo run --bin=dbgutil
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1
```

```
(qemu) info usbhost
  Bus 2, Addr 37, Port 3.2.1, Speed 5000 Mb/s
    Class ff: USB device 0b95:1790, AX88179
(qemu) device_add usb-host,hostbus=2,hostport=3.2.1
```
