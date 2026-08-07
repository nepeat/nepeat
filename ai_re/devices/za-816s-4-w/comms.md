# ZA-816S-4-W — architecture & board-to-board comms

How the two Ingenic T31 boards inside this one camera cooperate. Built from
static RE of both flash dumps (`dump/p25q64_GOOD.bin`, `dump/sensorboard_GOOD.bin`)
plus the units' own runtime logs recovered from the JFFS2 config/log partitions.
Cross-confirmed by three independent analysis passes.

## TL;DR

It's a **"gun-ball linkage" (枪球联动) auto-tracking camera** — two cameras in one:

- **BALL (master)** = board `JZ31_YT_YH2C_S01` — PTZ dome on a pan/tilt motor,
  its own sensor, **Hi3881 Wi-Fi**, person-detection AI, and the **cloud/RTSP
  gateway**. Runs `zrt_app` + `Daemon_app`.
- **GUN (slave)** = board `JZ31YT_YH0I_ICS_BOX_V1_3` — fixed wide **JXF37P**
  camera, encodes video, no Wi-Fi. Runs `rndis_server` (the imaging+link app).

The gun streams video to the ball; the ball runs person-detect and **steers its
motor to track** the target, and relays everything to the cloud over Wi-Fi.
Firmware is one family (Puwell / IPC365, v5.30.82.04, hostname `Zeratul`) built
in two roles. Supports up to 2 guns (2nd gun on `192.168.127.0/24`).

## Physical + link layer

```
   GUN  (board ICS_BOX)                              BALL (board YH2C)
   Ingenic T31 + JXF37P                              Ingenic T31 + Hi3881 Wi-Fi
   ┌────────────────────┐        USB cable           ┌────────────────────┐
   │ USB **gadget**     │  CDC-ECM / RNDIS ethernet   │ USB **host**       │
   │ usb0 = .128.16 ────┼───────── 192.168.128.0/24 ──┼──── usb0 = .128.15 │
   │ rndis_server       │                             │ zrt_app+Daemon_app │
   │ TCP LISTEN 12347   │◄───── ball connects in ─────│ TCP client         │
   │ TCP LISTEN 12351   │◄────────────────────────────│                    │
   └─────────┬──────────┘                             └─────────┬──────────┘
   JXF37P → ISP → H.264/5 ── video/motion push ──►  relay ──► Wi-Fi → IPC365 cloud
                          ◄── PTZ "shake" + ctrl ──   RTSP (PuWellRtsp) / P2P
```

- **Transport:** internal **USB**, run as a **USB-Ethernet gadget** (`g_ether`,
  CDC-ECM/RNDIS). Both `rcS` set `export TRANSFER_MODE=rndis`; the gadget/host
  plumbing lives in `zrt_dev_driver.ko` (on both boards), not a userland script.
  Confirmed by the gun's kernel log: `g_ether gadget: full-speed config #1:
  CDC Ethernet (ECM)`.
- **Addressing:** static, no DHCP on the link. **Ball = 192.168.128.15**,
  **Gun = 192.168.128.16** (`boardB rcS:65 ifconfig usb0 192.168.128.16 up`;
  ball sets .15 via ioctl). 2nd gun would be `192.168.127.16` ↔ `.127.15`,
  selected on the gun by a **GPIO60 + ADC hardware strap** (`check_gun_or_gun2`).
- Both IP literals appear in **both** images' binaries — the strongest proof of
  the intended pairing.

## Roles (who is who)

| | GUN — board ICS_BOX / sensorboard_GOOD.bin | BALL — board YH2C / p25q64_GOOD.bin |
|---|---|---|
| Linkage role | slave (fixed) | **master** (PTZ dome) |
| USB role | gadget/device | host |
| usb0 IP | 192.168.128.16 | 192.168.128.15 |
| TCP role | **server**, LISTEN **12347 + 12351** | **client** (connects in) |
| Wi-Fi at boot | none (depends on ball) | `wpa_supplicant` wlan0 → home AP → cloud |
| Sensor | **JXF37P** (kernel-detected; `sync.ini` default sc3235 is overridden by ball) | sc3235 (own camera) |
| Main app | `rndis_server` (imaging + link + `CShakeCtrl` motor) | `zrt_app` (AI, RTSP, P2P) + `Daemon_app` (link bridge) |
| Person-detect | yes (`libpersonDet_inf.so`, Ingenic jzdl NN) | yes (drives motor tracking) |

## Protocol over the link (gun = server, ball = client)

Ports **12347** and **12351** are the deployed values (from the gun's runtime
`host.log`: `enter tcp server port 12347/12351`, `Ready for rndis client conn`,
`[daemon] 192.168.128.15:59571 sock=9, listen port=12351`). Statically the port
is config-object-driven (not a string literal), so it comes from `/config` at
boot — these are what this unit actually used.

Channels multiplexed over the link (vendor calls the whole thing "rndis"):
- **Video/data:** gun `IMP_Encoder_GetStream` → `rndis_send2gun_image` /
  `rndis_recvgun_thread`; ball `rndis_h264_data_cb`, `P2PvideoGUNSendCallBack`.
  `send_motion_frame: data_conn=-1` in the gun log = ball's data socket not up.
- **PTZ / "shake" control:** `rndis_tcp_shake`, `CShakeCtrl`, `process data
  gun_shake`, `abs_target_degree[..]=>step[..]` — ball computes track target,
  sends degree/step motor commands to the gun's stepper.
- **Sensor bring-up handshake:** gun blocks at boot (`wait ball send sensor
  param....`) until the ball pushes `i2c_addr/name/resolution/bps`, then starts
  its ISP. **The ball is the master of record for the gun's imaging config.**
- **Heartbeat:** `rndis_gun_heart_data_proc`, `the heart beat stop with gun
  slave[%d]`; gun reboots if the ball is gone too long.
- **Upgrade/reboot:** `RNDIS_PKT_TYPE_UPGRADE_ACT`, `RNDIS_PKT_TYPE_REBOOT`,
  `GUN_DOWNLOAD_START`/`GUN_INSTALL_START`, `/tmp/gun.bin`. **The ball flashes
  the gun's firmware over the link.**

## Data flow (confirmed)
JXF37P → gun T31 ISP encode (H.264/H.265, 1080p main + 640×360 sub) → TCP push
over USB-net → ball `zrt_app` → { AI person-track → PTZ steer back to gun } +
{ RTSP `PuWellRtsp` / P2P / IPC365 cloud over Wi-Fi } + SD/cloud record.

## Security notes
- **No auth/pairing secret on the link** — the gun trusts the peer for sensor
  params, reboot, and **firmware flash**. Anything on `192.168.128.0/24` (i.e. a
  foothold on either board, or spoofing the USB-net) can push a reboot/flash to
  the gun. The "shake" handshake is protocol sync, not crypto.
- **Root serial console** on the gun's broken-out `R/T/G` pads: `getty` on
  `console` @ 115200 8N1 (`boardB rootfs/etc/inittab:30`).
- Shared `root` shadow hash across both boards (see `PROGRESS.md`).
- `/config/start.sh`, if present, overrides the boot app on either board →
  persistence foothold if `/config` is writable.

## Cloud / outbound (ball only)
IPC365 platform (`ca.ipc365.com` for this unit; regions sh/sg/sp/fr/ru), AWS
**DynamoDB** SigV4 backend, ONVIF + WS-Discovery server, RTSP realm `PuWellRtsp`,
P2P. Hardcoded AWS + Tencent IPs and resolvers `114.114.114.114` / `8.8.8.8`
baked into `zrt_app`. Full list in the Board A analysis notes.

Manufacturer/ODM = **Hangzhou Puwell OE Tech Ltd.** (Wi-Fi MAC OUI `40:6A:8E`).

### AWS credential flow (evidence, from `strings zrt_app`) — noted as a *possibility*
The ball is a first-class AWS client: it logs into IPC365
(`ZRT_Login_GetCredential`/`ZRT_GetCredentialToken`/`GetUploadCredential`),
receives **temporary creds** (`access_key_id` + `secret_access_key` +
`session_token` + `Expiration` — STS/Cognito-style), caches them at
`aws_file_path`, and signs **DynamoDB** calls itself (SigV4: `AWS4-HMAC-SHA256`,
`aws4_request`, `X-Amz-Security-Token`, `X-Amz-Target: DynamoDB_20120810.*`).
Ops seen: `UpdateItem`/`Query` writing a per-clip index
(`SET IndexPiece=:val1, IndexHeader=:val2, Expiration=:val3`, binary values).
**Unverified hypothesis (NOT tested — do not exercise):** if the vendor's IAM
policy doesn't scope those STS creds to this device's own key space, a rooted
unit could read/write other tenants' DynamoDB index (multi-tenant IDOR). Left as
a possibility only; the shared backend was deliberately not touched.

## Open / unresolved
- Exact USB host-vs-gadget electrical roles are inferred from software (kernel
  `zrt_dev_driver.ko` not disassembled).
- `192.168.128.1` also referenced (hub base vs gateway — unproven).
- Gun's `/config` (mtdblock3) app binaries (`start.sh`, its `zrt_app`) not yet
  cleanly extracted — the JFFS2 carve captured the `/log` partition. MTD map
  needs confirming from the kernel cmdline / U-Boot env.
