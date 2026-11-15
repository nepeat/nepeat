set -u
timeout 30 tcpdump -p -i eth0 -nn -e -c 600 -w /tmp/c.pcap 2>/dev/null
echo "frames: $(tcpdump -r /tmp/c.pcap -nn 2>/dev/null | wc -l)"
echo "--- by dst mac / ethertype"
tcpdump -r /tmp/c.pcap -nn -e 2>/dev/null | awk '{d=$4; sub(",","",d); e=$6; sub(",","",e); print d, e}' | sort | uniq -c | sort -rn | head -12
echo "--- by src mac"
tcpdump -r /tmp/c.pcap -nn -e 2>/dev/null | awk '{print $2}' | sort | uniq -c | sort -rn | head -6
echo "--- samples"
tcpdump -r /tmp/c.pcap -nn -e 2>/dev/null | head -12 | cut -c1-170
echo "--- drop delta over 10s"
a=$(cat /sys/class/net/eth0/statistics/rx_dropped); p=$(cat /sys/class/net/eth0/statistics/rx_packets); sleep 10
echo "dropped/s $(( ($(cat /sys/class/net/eth0/statistics/rx_dropped)-a)/10 ))  delivered/s $(( ($(cat /sys/class/net/eth0/statistics/rx_packets)-p)/10 ))"
echo "promisc: $(cat /sys/class/net/eth0/flags)"
lspci -v -s 0003:01:00 2>/dev/null | grep -E "^0|Memory at"
