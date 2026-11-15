al\_eth out-of-tree driver
==========================

`al_eth` is the driver for Amazon Annapurna Labs' Alpine NIC. Alpine SoCs are
used in a few consumer products, including some QNAP NAS appliances. This
driver source code was extracted from a Linux kernel source drop from QNAP.

The original source code was designed for Linux 4.2.8. The version in this
repository has only been tested with Linux 5.5.5.

TODO
----

* RX performance is a bit lackluster (~460Mbps).
* No 10Gbps support, or at least it's untested.
* Serdes driver should be split out into either an upstream-able module (in the
  Linux QNAP fork) or another out-of-tree module.
* Move some of the board specific stuff into DT (for example, clock/delay info
  currently hardcoded in a few places).
* Port the ethtool interface to more recent Linux APIs.
* Clean up coding style.

CS8210 (Alpine V3) changes
--------------------------

This copy came from dmc12-nixos (Alpine V2) and was changed for the Celestica
CS8210-32X's Alpine V3 integrated NIC:

* PCI revision 4 on the standard Ethernet unit is run as effective revision 2,
  which is what Annapurna's later HAL and the stock 4.9 kernel do.
* The MDIO bus registers clause-22 accessors unless the board says XMDIO; the
  management PHY (Marvell 88E1510, address 0) is a C22 device.
* phylib is attached with SGMII when the MAC mode is SGMII (was always RGMII-ID).
* A port that needs link management but has no SerDes object is refused at
  open time instead of oopsing in the link task. The V3 "AVG" SerDes is not
  ported, so the second NIC (00:03.0, 10GBASE-SR, no PHY) stays down.

Still built with `-DCONFIG_AL_ETH_ALPINE_V2`.
