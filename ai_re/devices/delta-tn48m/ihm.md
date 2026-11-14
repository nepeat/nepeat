# Amazon IHM device-identity ecosystem (as seen from the TN48M)

Passive analysis only (2026-11-11): files from this switch's factory-provisioned
dentOS install (`dumps/sda.img.zst`), read offline. No Amazon endpoint was
contacted and the device credentials were not used. "IHM" is not expanded
anywhere on the device; no public source found.

## org names seen

- **IHM**: Amazon org. Cert OUs `IHM` / `IHM Security`. Internal wiki namespace
  `w.amazon.com/bin/view/IHM/DPM/IdentityAgent/Manual`. Service names
  `IhmInfraCommodityZTP`, `IhmDeviceIdentity`, `IhmDeviceIdentityAgent`.
- **DPM**: a sub-team of IHM (wiki path `IHM/DPM`, dpkg maintainer
  `ihm-dpm@`, CAs `O=DPM, OU=Provisioning`). The code calls its client
  `DeviceProvisioningClient*`, so "Device Provisioning ..." is likely, but
  unconfirmed.
- **Aureus**: CN of the NEMO / ORVILLE / ECO provisioning CAs (OU=IHM Security).
  It looks like the name of IHM's device-CA service; the meaning is unknown.
- Stages: every CA family comes as alpha/beta/gamma/prod (Amazon's usual
  deployment stages).

## device families (from the trust store)

All are ECDSA P-256 CAs with 20-year lifetimes, except the OIDC/DPM roots (RSA).

| family | CA CN | issued | what it is |
| --- | --- | --- | --- |
| ECO | Aureus (beta/gamma/prod) | 2019-02-20 | unknown |
| ORVILLE | Aureus (beta/gamma/prod) | 2019-03-22 | unknown |
| **NEMO** | Aureus (beta/gamma/prod) | 2019-03-26 | a camera (per the user, who has one) |
| Networking_Switch | Networking_Switch Provisioning CA (beta/gamma/prod) | 2020-05-08 | this switch (DENT TN48M) |
| OPC | OPC Provisioning CA (beta/prod) | 2020-05-07 | unknown |
| IHM OIDC | IHM OIDC Root CA (ACM alpha/beta) | 2020-07-01 | OIDC token issuer for workloads |
| DPM | provisioning-ca.beta.pem, auto-provisioning-ca.test.pem | 2019-01 / 2019-11 | provisioning service (beta/test) |
| public | Amazon Root CA 1-4, Starfield G2, VeriSign G5 | — | TLS to AWS endpoints |

NEMO CA fingerprints (sha256), for matching against the camera:

- prod  `0E:8F:E8:A9:3C:0C:16:5D:17:DD:29:A9:54:9F:23:63:02:F3:34:7F:8E:19:F1:14:51:4F:4F:40:9D:0B:52:AD`
  (SKI `88:83:A6:7D:4F:21:B8:35:AA:23:9F:95:23:D1:7E:C5:26:84:B4:EC`)
- gamma `2A:07:1E:D0:07:7F:DC:DF:E0:3A:0A:60:6F:9C:AB:8A:5C:3F:2E:1A:D0:8F:46:7E:C2:F0:8B:9A:4C:AA:80:F7`
- beta  `CC:45:57:6B:CF:AE:93:F4:5F:81:A6:5D:B9:6F:EA:EA:FF:7C:E4:2A:31:9E:7F:B4:CE:74:8F:A5:BA:8B:ED:FA`

Networking_Switch prod: `0F:5A:F7:67:1A:06:CC:B4:57:E3:52:BF:0E:80:FC:AF:45:53:1F:15:B6:A2:83:B9:E8:14:53:63:9A:E5:B6:29`.

## on-device components (dentOS packages maintained by `ihm-dpm@`)

| package | contents |
| --- | --- |
| identity-agent | `device-identity-agent` (16.7 MB, static C: libcurl, c-ares, OpenSSL), `libdevice-identity.so`, systemd units for agent/hostname/interface |
| identity-client | `device-identity-client` CLI + lib (`--thing-id`, ...) |
| identity-agent-dent | DENT/ONL glue: firstboot moves state to the ONL-CONFIG partition and bind-mounts it at `/mnt/identity-agent`; `identity-agent-unconfig` wipes it; TPM permissions |
| bridge-agent | `device-bridge-agent` (16.8 MB), `libdevice-bridge.so`, `bridge-interface.sh` (fake IMDS + iptables), `bridge-nanny.sh`, `sepes-adapter.sh` |
| bridge-client | `device-bridge-client` CLI + lib (`--credentials`, `--business-credentials [--legacy]`) |

Plus `/etc/init.d/ztp_boot.sh`: zero-touch provisioning run once per boot by
`IhmInfraCommodityZTP` until a ZTP script succeeds ("attempts to emulate the
way Cumulus devices use ONIE"). It logs to `/var/log/autoprovision`.

State layout (`/mnt/identity-agent/storage/`): `trust/` (CA bundle above),
`persist/` (identity key + cert, `idme.properties`), `ephemeral/`.

## the identity

- At provisioning the agent generates a key **in the TPM** (`TSS2 PRIVATE KEY`,
  `storage.engine.identity=hardware-tpm`) and a CSR with subject
  `O=Amazon, OU=<country>, OU=<site>, OU=Provisioning, OU=<thing type>, CN=<thing id>`
  and custom extensions in the private `1.2.3.4.x` arc:
  `.2` = PSN (`DENT_TN481N1TW21290148`), `.3` = model (`DENT_TN48M`),
  `.4` = asset id (`3811100345`, = the ONIE TLV service tag), `.5` = mgmt MAC.
- The thing-type CA signs it (here "Networking_Switch Provisioning CA (prod)",
  2021-07-21 08:02 UTC at DNI Taiwan), valid 20 years.
- `idme.properties` mirrors **Fire OS IDME** naming (`ro.idme.serial`,
  `ro.idme.psn`, `ro.idme.provisioning.thing_id/_type`,
  `ro.idme.manufacture.country/site`; cf. `devices/amzn-pinnacles`, whose Fire
  tablet has a real IDME block). The same identity scheme seems to stretch from
  consumer devices to infrastructure gear.
- `/etc/default/identity-agent`: `MANUFACTURE_SITE/COUNTRY`, `STORE_DOMAIN`,
  `STORE_ID` (both `DNI` = factory placeholder), `THING_TYPE`,
  `IDENTITY_KEY_TYPE=hardware-tpm`. Devices are evidently assigned to
  **stores** later; `identity-hostname.sh` renames the host to the thing id.
- References `iibs-midway.corp.amazon.com` (Midway = Amazon corporate SSO).

## the bridge (credential vending)

`/etc/default/bridge-agent` (generated 1602714412 = 2020-10-14):
`METADATA_ENABLE=true`, `STATSD_ENABLED=true`,
`BRIDGE_CREDENTIAL_STRATEGY=IOT,OIDC`, `BRIDGE_OIDC_ENABLE=true`.

Credential paths (format strings in the binaries):
1. **IoT**: mTLS with the TPM identity to the AWS IoT credentials provider,
   `https://%s:%d/role-aliases/%s/credentials` (`credentials.iot.address`,
   `credentials.role_alias`). Endpoint in the binary:
   `a2d7hi2rx9opq4.iot.us-west-2.amazonaws.com`.
2. **OIDC**: `/idp-authorization` + `/idp-token` (IHM OIDC root CA), then STS
   `AssumeRoleWithWebIdentity` at `sts.us-west-2.amazonaws.com`
   (`credentials.sts.*`).
3. **Business role**: `GetAssumeRoleCredentials?roleARN=%s&duration=%d` from a
   "runtime credential provider service", giving a second, per-business role
   (`business.role.arn`, `business.role.tag.enable`, tagged with store
   domain/id). Exposed as `credentials.business.aws.*`.
4. Served to local workloads through an **EC2-IMDS emulation**:
   `169.254.169.254` on lo, DNAT :80 -> :8080, only root/`system`/docker
   allowed, path `/latest/meta-data/iam/security-credentials/`.
- `bridge-nanny`: every 15 min, check `device-bridge-client --credentials` (a
  "good proxy for the security hardware"); 8 strikes in a row and it exits.
- `sepes-adapter`: every 5 min writes business credentials to
  `$SEPES_CREDENTIALS_FILE` for a consumer called SEPES (unknown).

## what to look for on the NEMO camera

- A device cert chain ending in `CN=Aureus, OU=IHM Security` with one of the
  NEMO fingerprints above; `idme`/`ro.idme.*` properties with
  `thing_type` and `store` fields; an identity-agent / bridge-agent (or an
  embedded equivalent) and a TPM/secure element holding the key.
- The same passive rule applies: read what's on the device, don't present its
  identity to Amazon services.
