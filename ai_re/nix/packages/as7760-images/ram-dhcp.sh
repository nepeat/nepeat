#!/bin/sh
# Disposable probe network configuration; all files live in RAM.
case "$1" in
  deconfig) ip addr flush dev "$interface" ;;
  bound|renew)
    /bin/busybox ifconfig "$interface" "$ip" netmask "$subnet" up
    if [ -n "$router" ]; then
      set -- $router
      ip route replace default via "$1" dev "$interface"
    fi
    : > /etc/resolv.conf
    for server in $dns; do echo "nameserver $server" >> /etc/resolv.conf; done
    ;;
esac
