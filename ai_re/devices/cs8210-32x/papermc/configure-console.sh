#!/usr/bin/env bash
set -eo pipefail
cd /root/papermc
systemctl stop papermc
umask 077
password=$(sed -n 's/^rcon.password=//p' server.properties)
if [ -z "$password" ]; then
    password=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
fi
sed -i '/^enable-rcon=/d; /^rcon.password=/d; /^rcon.port=/d; /^white-list=/d; /^enforce-whitelist=/d' server.properties
printf '\nenable-rcon=true\nrcon.port=25575\nrcon.password=%s\nwhite-list=false\nenforce-whitelist=false\n' "$password" >> server.properties
chmod 600 server.properties
systemctl start papermc
