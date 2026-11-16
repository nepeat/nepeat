# bgpmon over all VRFs (patch 0006): "show bgp vrf all summary json", default-VRF
# neighbors as NEIGH_STATE_TABLE|<ip>, the others as NEIGH_STATE_TABLE|<vrf>|<ip>.
import json
from unittest.mock import patch

from bgpmon.bgpmon import BgpStateGet


def peers(**p):
    return {"peers": {ip: {"state": st, "remoteAs": ra, "localAs": la} for ip, (st, ra, la) in p.items()}}


VRF_ALL = {
    "default": {"ipv4Unicast": {"peers": {"10.0.0.1": {"state": "Established", "remoteAs": 65100, "localAs": 65100}}}},
    "Vrf-a": {"ipv4Unicast": {"peers": {"10.0.1.1": {"state": "Established", "remoteAs": 65201, "localAs": 65100}}},
              "ipv6Unicast": {"peers": {"fc00::1": {"state": "Active", "remoteAs": 65201, "localAs": 65100}}}},
    "Vrf-b": {"ipv4Unicast": {"peers": {"10.0.1.0": {"state": "Connect", "remoteAs": 65101, "localAs": 65100}}}},
}


@patch('bgpmon.bgpmon.swsscommon.RedisPipeline')
@patch('bgpmon.bgpmon.swsscommon.SonicV2Connector')
def mon(conn, pipe):
    conn.return_value.STATE_DB = 'STATE_DB'
    return BgpStateGet()


def run(m, out):
    flushed = {}
    with patch('bgpmon.bgpmon.getstatusoutput_noshell', return_value=(0, json.dumps(out))) as cmd, \
         patch.object(BgpStateGet, 'flush_pipe', lambda self, data: (flushed.update(data), data.clear())):
        m.get_all_neigh_states()
        m.update_neigh_states()
    return cmd.call_args[0][0], flushed


def test_all_vrfs_keys_and_states():
    m = mon()
    cmd, data = run(m, VRF_ALL)
    assert cmd[-1] == 'show bgp vrf all summary json'
    assert data == {
        "NEIGH_STATE_TABLE|10.0.0.1": {"state": "Established", "peerType": "i-BGP"},
        "NEIGH_STATE_TABLE|Vrf-a|10.0.1.1": {"state": "Established", "peerType": "e-BGP"},
        "NEIGH_STATE_TABLE|Vrf-a|fc00::1": {"state": "Active", "peerType": "e-BGP"},
        "NEIGH_STATE_TABLE|Vrf-b|10.0.1.0": {"state": "Connect", "peerType": "e-BGP"},
    }


def test_vrf_neighbor_gone_and_state_change():
    m = mon()
    run(m, VRF_ALL)
    later = json.loads(json.dumps(VRF_ALL))
    del later["Vrf-a"]["ipv6Unicast"]
    later["Vrf-b"]["ipv4Unicast"]["peers"]["10.0.1.0"]["state"] = "Established"
    _, data = run(m, later)
    assert data == {"NEIGH_STATE_TABLE|Vrf-b|10.0.1.0": {"state": "Established", "peerType": "e-BGP"},
                    "NEIGH_STATE_TABLE|Vrf-a|fc00::1": None}


def test_single_vrf_form_still_parsed():
    m = mon()
    _, data = run(m, VRF_ALL["default"])
    assert data == {"NEIGH_STATE_TABLE|10.0.0.1": {"state": "Established", "peerType": "i-BGP"}}
