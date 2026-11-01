# Evidence policy

Read with [AGENTS.md](../AGENTS.md) before collection/import. Source-specific
notes and saved fetch reports remain authoritative for details and access outcomes.

## Preserve what the source actually establishes

- Keep `sources/` read-only. Original raw captures are immutable; save changed
  versions separately with original URLs, dates, SHA-256 hashes and capture method.
- Preserve full model prefixes, suffixes, revisions, case-as-printed and conflicting
  assertions. Regulatory codes, families and commercial SKUs need explicit source
  evidence before linking them; do not expand grouped shorthand into invented SKUs.
- Brand, OEM/manufacturer, applicant, grantee, listing holder, factory, platform
  vendor, parent company and operator are separate roles. An exact company-name
  association is not proof of physical manufacture. Review dated entity relationships
  before mapping aliases or parent/subsidiary portfolios. See the
  [target registry](../research/odm-target-registry.md).
- Amazon consumer entities are not automatically Amazon Data Services. Design
  lineage, deployment plans, dated deployment and the origin of a specific auction
  unit are different claims. Owner-reported AWS history stays owner context.
- Seller text alone does not establish identity proof. Marketplace search links
  are leads; listing mentions do not establish availability, condition, configuration,
  entitlement, manufacture or previous ownership of the item being sold.
- Historical compatibility/firmware lists are source claims, not current support
  guarantees or recommendations to flash an auction unit. Preserve absent facts.

## Validation is not evidence

[Owned hardware](../validation/owned-hardware.json) contains ten expectations.
Nine have document snapshots; `7760-32X-A-AC-F` uses a reviewed chassis-label photo
that explicitly pairs it with `AS7760-32X`. Keep that 9+1 distinction visible.
Preserve the full `AZ4444-52X-DC-10` name. Do not import validation lists as sources.

[ByteDance B4020/B4010](../validation/vendor-examples.json) are separate coverage
examples, without an ownership or ByteDance-manufacturing assertion. A matching
Accton applicant model does not validate ByteDance. See
[the source review](../research/bytedance-inspur-2026-10-04.md).

## Discovery and distinct identities

Sitemap URLs, identifiers, PCI IDs and MAC OUIs are discovery/reference data, not
automatic device, application or manufacturer assertions. The raw Inspur support
feed mixes systems, third-party parts, software and categories; retain full names,
duplicates, nullable metadata and parent IDs. Neither a leaf flag nor a model-like
name establishes an Inspur brand/manufacturer claim.

Keep chassis, node, appliance and component identities separate. In particular:

- `i24` is not an alias of `NS5162M5`; InCloud Rail appliance and server-node
  columns are not aliases. Table vendor names do not establish physical manufacture.
  [Inspur table review](../research/inspur-rail-2026-10-04.md).
- `FS6500/6600` and `AS13000-M1/M3/M5/Rack` remain grouped. NF5288M5/AGX-2 has
  an explicit source-scoped alias. M13 memory, NF5288M5 PSU and X680m memory
  conflicts remain unresolved. The SR-AI/Baidu joint-design claim does not transfer
  to separate nodes or establish deployment.
  [2018 catalog review](../research/inspur-2018-inventec-2026-10-04.md).
- Wiwynn `SV7220G3-V+` differs from V; the printed plus can detach during text
  extraction. Hyve Hercules has conflicting Orion prose; its HS9124 motherboard
  is separate. JBOF's unusual voltage text must not be silently fixed. See the
  [Wiwynn/Hyve review](../research/wiwynn-hyve-2026-10-04.md).
- Inspur NICs use a compatibility-table vendor role. Duplicate storage/management
  rows, three blank remarks and the X722 contradiction remain separate source
  statements. See the [NIC review](../research/inspur-nics-2026-10-04.md).

## Access and acquisition boundaries

Use public/free and open-source sources. The paid Datadory path is closed; no
quotes, samples, subscriptions or provider/seller outreach are authorized.
Consult saved fetch reports before requests. Do not repeatedly retry known 402/403
or explicit restrictions, or fetch the same denied item by another route. Preserve
unavailable versus empty and stop/checkpoint on throttling. Gated/private serial,
warranty, activation and contact endpoints are outside ordinary public collection.

Raw/exported datasets keep upstream rights and may contain public regulatory
contact information; local storage does not authorize publication. Do not delete
historical snapshots or weaken evidence checks to make an import pass.
