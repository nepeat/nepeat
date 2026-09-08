#!/system/bin/sh
# Persistent root command worker. Start once through the modprobe helper, then
# let that helper exit before submitting any shutdown-capable command.
set -u

base=/data/local/tmp/yacht-rootd
mkdir -p "$base"
rm -f "$base/ready" "$base/running"
echo $$ >"$base/pid"
id >"$base/ready"

while :; do
    if [ -f "$base/cmd" ]; then
        mv "$base/cmd" "$base/running"
        token=$(sed -n '1p' "$base/running")
        sed '1d' "$base/running" | sh >"$base/out.new" 2>&1
        rc=$?
        mv "$base/out.new" "$base/out"
        echo "$token $rc" >"$base/status.new"
        mv "$base/status.new" "$base/status"
        rm -f "$base/running"
    fi
    sleep 0.1
done
