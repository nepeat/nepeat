# bytedance finite discovery handoff

outcome: bounded pass complete; primary affiliation evidence remains missing for
both B4020 and B4010. no curated input or import proposed.

## artifacts and checks

- raw captures, two full search-result responses and extraction:
  `data/raw/vendor-discovery-bytedance-2026-10-04/`.
- `capture-report.json` records the eight exact queries, outcomes, capture time,
  methods, sizes and sha-256 hashes. all stored capture hashes verified; three
  tool-output JSON files parsed; original HTML checked for exact `B4020`.
- 8/8 searches used; 2/6 source fetches used (web extraction plus original HTML
  capture of the same newly discovered article). no code, database, manifest,
  complete PDF bundle or exports modified. no tests needed for research-only work.

## supported source scope and exclusions

existing `data/raw/bytedance-inspur-2026-10-04/accton-b4010-compliance.pdf`
remains the only reviewed primary exact-model evidence in this pass: B4010,
Ethernet Switch, brand N/A, Accton Technology Corporation applicant, report
BTL-FICE-1-2302C014. existing source note and curated record were read; no original
capture altered or re-requested. applicant is not manufacturer or ByteDance.

new discovery: community article [智算中心整体网络架构](https://tencentcloud.csdn.net/69ea55a10a2f6a37c5a3eb0b.html),
author handle 阿拉斯加大闸蟹. the visible publication timestamp is
2026-01-22 15:13:39, without a displayed zone; saved original HTML explicitly
gives `bytedance:published_time` as `2026-01-22T15:13:39+08:00` and
`bytedance:updated_time` as `2026-04-24T01:23:45+08:00`. these metadata keys are
a publisher convention and do not establish ByteDance affiliation. web extraction
lines 318–320 state under 字节大模型组网 that earlier
B4020 switches provide 64*400GE and later B5020 provides 64*800GE. this is a
secondary community claim only. the article itself says architectures derive
from public information but its author has refined them and individual details
may diverge from reality (line 193); it supplies no specific original ByteDance
source for B4020. Tencent/CSDN hosting is not a ByteDance assertion or official
product datasheet. neither hardware specifications nor affiliation promoted.
B5020 is outside the assigned exact-model set and retained as an optional lead.

Volza and Eximpedia search previews repeat exact ByteDance B4010/B4020 trade
labels, but remain existing secondary trade-data leads. Eximpedia's saved 402 was
respected: no opening, retry, alternate route or paid data access. search-engine
results incidentally returned that known URL; a snippet is not a new fetch of it.
no shipment origin, exporter, buyer, manufacture or deployment inferred.

other results were model-string collisions: peptides, building IDs, Toyota parts,
TwinCAT build versions, crayons, waste classifications and unrelated products.
site-restricted queries for SONiC/GitHub, OCP, Accton B4020 and ByteDance B4010
yielded no qualifying source in these saved results. this establishes a bounded
search gap, not absence across those sites or worldwide archives.

## next finite step

keep ByteDance examples at 0/2. a later bounded pass can trace the community
diagram's original conference presentation or official ByteDance networking
publication (at most three original presentation/product sources), checking
exact B4020/B4010 printed labels and affiliation. do not import the community
topology or trade preview to fill the validation gap. user-supplied chassis-label
or original compliance/product documentation would also be distinct evidence;
no seller/provider outreach follows from this handoff.
