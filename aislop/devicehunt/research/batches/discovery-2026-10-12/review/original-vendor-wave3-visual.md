# coordinator visual review — original-vendor wave 3

/root independently viewed four local renders on 2026-10-12 PDT: SD6200
PDF pages 1–2 and WEDGE100-32X PDF pages 2–3. These are discovery-level
visual notes, not candidate acceptance.

SD6200 page 1 explicitly names the 4U 90-HDD SAS-4 expansion system. The
feature table specifies up to 90 12G SAS or 6G SATA 3.5/2.5-inch drives,
13 W maximum drive power at 35°C, dual redundant hot-swappable ESMs,
single-ESM support, and four 24G SAS4 mini-SAS HD ports per ESM. Preserve
drive-side and ESM port ratings separately; do not infer host/cable compatibility.

SD6200 page 2 specifies 1300 W 80-plus Platinum PSUs, 2+2 at 200–240 V or
3+1 at 100–240 V, redundant and hot-swap. The separate AC-input statement
is 100–127/200–240 V, auto-ranging, 47–63 Hz. Five 80 mm dual-rotor fans are
hot-pluggable from the top. Dimensions: height 175.3 mm, width 448 mm,
depth 1070 mm with CMA. Operating temperature is 5–35°C; altitude 0–950 m,
with temperature derated 1°C per 300 m above 950 m. No weight or rack-depth
fitment is inferred. Footer: copyright 2024, code 091024.

Wedge page 2 specifies two redundant, load-sharing, hot-swappable AC or
-54 VDC PSUs; printed input is 120–240 VAC at 50–60 Hz and 40–72 VDC.
A separate power-input option is 12 VDC. Typical consumption is 436 W.
Preserve the polarity/range wording. W×D×H is 44×50.7×4.4 cm, weight 8.8 kg,
with 4+1 hot-swappable fans and front-port to back-power airflow. Operating
temperature is 0–45°C. Historical software prose says OpenBMC and diagnostics
are loaded and lists FBOSS compatibility and ONL, FlexSwitch and Cumulus 3.4+
options; this establishes no current support or entitlement. The Accton
manufacturing paragraph concerns the generic Edgecore data-center-switch
family and needs separate role-scope review before a new affiliation.

Wedge page 3 prints the complete individual ordering codes:
Wedge100-32X-O-12V-F, Wedge100-32X-O-AC-F-US, Wedge100-32X-O-AC-F-EU,
Wedge100-32X-O-AC-F-UK and Wedge100-32X-O-AC-F-JP. The first description,
“12V power supply,” does not alone establish that the full code names a
standalone PSU FRU. Regional descriptions identify US/EU/UK/JP SKU power
cords of Wedge; they do not authorize general suffix decoding. ORSA-1U is
separately described as an Open Rack Switch Adapter for Open Rack installation.
Footer code: EC-DS-0617-04.

The coordinator made no additional render or network requests. Hash bindings
follow; captured URLs, status and request caps belong to the author's ledger.

- `sd6200-1.png`: `f6413d21b7955749674b41ef74acc359097ada58255bd5ccbed9b8cc0597d687`
- `sd6200-2.png`: `585e574c353ea5a0976b03737b4f8d0b0425f9a58825c2f6ff383800162e5eff`
- `wedge100-2.png`: `61b517fb3741f987733466856637e33fca992bd8ecba3ca35e4bc669c8dffc98`
- `wedge100-3.png`: `dbb7c6fa6e5ec7484521f01604c32e728bbad40a8be79a9a8027eff2d4fdfcea`
- `sd6200-datasheet.pdf`: `3a92bffdaafff4b4428c5ac1d4b6514c347154ca828b65639217837c4efa2f54`
- `wedge100-32x-r04.pdf`: `66368b4e16367b335f27d53c6819d884e15e53390dbcc8e6528c39024fda4269`
