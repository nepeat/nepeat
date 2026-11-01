# AWS WorkSpaces Thin Client: separate Amazon entities

The public [AWS safety index](https://aws.amazon.com/workspaces-family/thin-client/terms/safety-and-compliance-information/)
links both PDFs captured on September 26, 2026 (local date). The index HTML,
original PDFs and complete embedded-text captures are retained in `data/raw`
as `aws-thin-client-*` and hashed in the offline import manifest.

The EU declaration names **GA5Z9L**, describes a digital media streaming device,
and names **Amazon.com Services LLC** as manufacturer. Page 2 prints August 29,
2022 and contains a visible signature; this review does not authenticate the
signature or establish current conformity. Both pages were visually reviewed.

The safety guide, whose URL labels it a March 2025 update, explicitly pairs
**Amazon WorkSpaces Thin Client Device** with **GA5Z9L** on pages 3 and 4.
Page 3 names **Amazon.com Services, Inc.** as FCC responsible party and EU/UK
declarant. Those printed names are preserved without resolving the LLC/Inc.
difference. Neither document establishes Amazon Data Services as manufacturer.
The guide references FCC-ID lookup but does not print the device's FCC ID in
its extracted text; no ID is constructed from the model.

Pages 3–5 were visually reviewed. The page 4 table prints 12 VDC, 1.25 A and
0–35°C; its separate maximum-power row prints 4.17 W. These remain source text,
without equating the device rating with measured power. Page 5 says an AWS
account is required. No inference is made about converting firmware, reuse
outside that service, other products sharing a model code, physical factories,
or hyperscaler deployment.

These are document-discovery records only, outside the target-company identity
catalog. The generic reference importer archives seven pages, all with embedded
text, under `reference-pdf-aws-thin-client-eu` and
`reference-pdf-aws-thin-client-safety`. No aliases, affiliations, FCC filings or
owned-hardware validation passes are created.

```sh
python3 -m devicehunt documents GA5Z9L
python3 -m devicehunt documents --source reference-pdf-aws-thin-client-safety --limit -1
python3 -m devicehunt lookup GA5Z9L
```

The model query returns three pages across the two documents. Results are saved
in `data/exports/documents-thin-client.json`. SQL users can query `document_text`
with `text LIKE '%GA5Z9L%'`, retaining its `source_id` and `page_number`.
