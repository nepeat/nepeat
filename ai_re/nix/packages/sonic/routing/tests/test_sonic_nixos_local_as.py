# sonic-nixos: BGP_NEIGHBOR local_asn on live peers (patch 0005) and for new
# peers (template patch 0004). The template cases are data files next to
# upstream's (tests/data/general/instance.conf/param_local_as_*.json).
from unittest.mock import patch

from .test_bgp import constructor, load_constant_files

LOCAL_AS = {"local_asn": "65101", "local_as_no_prepend": "true", "local_as_replace_as": "true"}


def pushed(m):
    return [c.args[0] for c in m.cfg_mgr.push.call_args_list]


def test_local_as_args():
    f = constructor(load_constant_files()[0]).local_as_args
    assert f({}) is None
    assert f({"local_asn": "65101"}) == "65101"
    assert f({"local_asn": "65101", "local_as_no_prepend": "true"}) == "65101 no-prepend"
    assert f(LOCAL_AS) == "65101 no-prepend replace-as"
    # FRR takes replace-as only after no-prepend
    assert f({"local_asn": "65101", "local_as_replace_as": "true"}) == "65101"
    for bad in ("", "0", "-5", "65101\n neighbor 1.1.1.1 shutdown", "1.10", None):
        assert f({"local_asn": bad}) is None, bad


@patch('bgpcfgd.managers_bgp.log_err')
def test_local_as_set_change_remove_live(mocked_log_err):
    for constant in load_constant_files():
        m = constructor(constant)
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("10.10.10.1", dict(LOCAL_AS))
        assert pushed(m) == ["router bgp 65100\n bgp suppress-fib-pending\n neighbor 10.10.10.1 local-as 65101 no-prepend replace-as\nexit"]
        assert not [c for c in mocked_log_err.call_args_list if "Can't update" in c.args[0]]
        # same values again: nothing to do (and no "Can't update the peer")
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("10.10.10.1", dict(LOCAL_AS))
        m.cfg_mgr.push.assert_not_called()
        mocked_log_err.assert_called_with("Peer '(default|10.10.10.1)': Can't update the peer. Only 'admin_status' attribute is supported")
        mocked_log_err.reset_mock()
        # flags dropped: plain local-as
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("10.10.10.1", {"local_asn": "65102", "admin_status": "up"})
        assert "router bgp 65100\n bgp suppress-fib-pending\n neighbor 10.10.10.1 local-as 65102\nexit" in pushed(m)
        # removed
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("10.10.10.1", {"admin_status": "up"})
        assert "router bgp 65100\n bgp suppress-fib-pending\n no neighbor 10.10.10.1 local-as\nexit" in pushed(m)
        assert not [c for c in mocked_log_err.call_args_list if "Can't update" in c.args[0]]


def test_local_as_in_vrf_live():
    for constant in load_constant_files():
        m = constructor(constant)
        m.peers.add(("Vrf-b", "10.0.1.0"))
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("Vrf-b|10.0.1.0", dict(LOCAL_AS, admin_status="up"))
        assert "router bgp 65100 vrf Vrf-b\n bgp suppress-fib-pending\n neighbor 10.0.1.0 local-as 65101 no-prepend replace-as\nexit" in pushed(m)


def test_local_as_new_peer_rendered():
    for constant in load_constant_files():
        m = constructor(constant)
        m.cfg_mgr.push.reset_mock()
        assert m.set_handler("30.30.30.1", dict(LOCAL_AS, asn="65201", local_addr="30.30.30.30", name="TOR", bfd="true"))
        out = "\n".join(pushed(m))
        assert " neighbor 30.30.30.1 remote-as 65201\n" in out, out
        assert " neighbor 30.30.30.1 local-as 65101 no-prepend replace-as\n" in out, out
        assert " neighbor 30.30.30.1 bfd\n" in out, out
        # local-as comes before the BFD line, so bgpd picks the session type with the final AS
        assert out.index("local-as 65101") < out.index("neighbor 30.30.30.1 bfd"), out
