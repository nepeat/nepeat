# local PDF visual checks — wave 2

Reviewer `/root`, independent of capture/extraction author `/root/psu`.
Viewed three locally rendered PNGs with `view_image`, from the immutable saved
PDF bytes using existing Poppler 26.06.0. No new HTTP, OCR or inferred fitment.
Render provenance: `../pdf-runtime/runtime-report.json` and
`../psu-visual/render-manifest.json`.

## CSA PFE1500 certificate, PDF page 4

Viewed `../pdf-runtime/bel-pfe1500-csa-page4.png`. Header visibly gives certificate
80005631, project 80111077 and issue 2022-01-10. The third table on the page is
preceded by a model list explicitly including PFE1500-12-054NAC and scoped to
Canada/United States. It specifies main V1 = 12.0 V and linear derating with
ambient increase 45°C → 60°C and input decrease 200 VAC → 100 VAC.

| input / scope | 45°C | 60°C |
| --- | --- | --- |
| 100–200 VAC, Canada/US model group including NAC | 83.4–125 A | 69–104 A |
| 200–240 VAC or 200–350 VDC, same group | 125 A | 104 A |
| 100–200 VAC, outside Canada/US group explicitly naming NAC/NACS412/RAC | 66.5–125 A | 66.5–104 A |
| 200–240 VAC or 200–350 VDC, same outside-Canada/US group | 125 A | 104 A |

These two adjacent model groups and both temperature columns support the prior
discovery note. Other tables on the page name different suffixes and have lower
ratings; those values are not transferred to NAC. No current certification
validity or rating for every possible input/ambient combination is asserted.

## PFE1100 datasheet, PDF page 9

Viewed `../psu-visual/bel-pfe1100-page9.png`; footer BCD.00012_AP, copyright 2023.
Figure 20 caption explicitly names PFE1100-12-054NA; figure 21 names the separate
RA model. Figure 20 plots nominal main-output current against AC input voltage
for four ambient bands (<35, <45, <55, <65°C). Thus low-input/temperature
dependence is visually established for NA. The prose distinguishes actual
current limitation from figure-20 nominal current by approximately 4 A.
Do not equate a trip threshold with the rated output, transfer RA's graph to NA,
or claim exact numeric curve readings without further reviewed transcription.
No new interpolated graph points are accepted here.

## FSP550-29FM filename, PDF page 2

Viewed `../psu-visual/fsp550-29fm-page2.png`. The visible page header unmistakably
says **FSP550-20FM**, followed by mechanical specifications, dimensions and an
airflow arrow. This confirms the source conflict noted in extracted text.
The page-2 drawing/airflow is excluded from claims about FSP550-29FM; page-1
model/electrical statements require their own page review. Neither filename nor
shared series context resolves the conflict.

Outcome: the three specifically requested table/graph/conflict checks are now
complete. This is not an all-pages review or import approval. Complete extraction,
remaining fact-bearing page review, honest extractor metadata, candidate evidence
review and normal integration checks remain before catalog promotion.
