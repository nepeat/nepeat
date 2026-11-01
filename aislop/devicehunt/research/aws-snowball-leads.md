# AWS Snowball model research

Reviewed 2026-09-26. This is a research note, not catalog evidence or a
manufacturer assertion. No new Snowball identity was imported during this pass.

The official [device identification guide](https://docs.aws.amazon.com/snowball/latest/developer-guide/identifying-device.html)
documents two values returned in the `DeviceType` field of `describe-device`:

| Software-reported type | Documented configuration |
|---|---|
| V3_5C | Snowball Edge compute-optimized with AMD EPYC Gen2 and NVME |
| V3_5S | Snowball Edge storage-optimized 210 TB |

These are useful discovery terms. This page does not establish that they are
printed chassis model numbers, regulatory model numbers, or manufacturer part
numbers. The example `DeviceId` is an illustrative device-specific identifier,
not a catalog model. No alias, physical manufacturer or factory relationship
is inferred from these fields.

The linked [hardware information page](https://docs.aws.amazon.com/snowball/latest/developer-guide/device-differences.html)
provides configuration-specific specifications. It distinguishes average power
consumption from the power supply rating. A future import should retain that
distinction and qualify facts by configuration instead of attaching them to
every historical Snowball device. Neither page reviewed here names a legal
manufacturer or identifies a printed chassis code.

The already cached Snow safety index links a September 2022 family guide and
Snowcone-specific regional documents. Those links did not supply a new
Snowball model declaration in this pass. The existing AF1 and AF1 BD1 evidence
remains scoped to Snowcone; see [the Snowcone evidence note](aws-snowcone.md).

Next useful evidence would be a public original declaration or readable label
that explicitly pairs a Snowball chassis code with its manufacturer and product
family. This limited search does not establish that such evidence is absent.

The owned-hardware validation was rerun after this research: all four Oracle
expectations pass; overall 10/10 queries return results and 8/10 exact identities
are independently supported. The two existing Edge-Core gaps remain open.
