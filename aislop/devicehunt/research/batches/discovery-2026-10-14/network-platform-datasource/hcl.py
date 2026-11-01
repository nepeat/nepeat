"""Offline, strict HTML hardware-matrix extraction and source-scoped candidates.

No network calls. HTML cells retain printed case, qualifiers, duplicates and links.
The generic table reader can serve later matrices; adapters must validate their own
headers and semantics rather than guessing aliases or expanding grouped names.
"""
from __future__ import annotations

import argparse
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import sqlite3
from urllib.parse import parse_qs, urljoin, urlsplit


def clean(value):
    """Normalize HTML layout whitespace only; leave punctuation and case intact."""
    return " ".join(value.split())


class MatrixHTML(HTMLParser):
    """Read explicit, rectangular tables; reject ambiguous cells and nested tables."""

    def __init__(self, base_url):
        super().__init__(convert_charrefs=True)
        self.base_url = base_url
        self.tables = []
        self.table = self.row = self.cell = self.heading = None
        self.headings = {}
        self.skip = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag in ("script", "style"):
            self.skip += 1
        if self.skip:
            return
        if tag in ("h1", "h2", "h3", "h4", "h5", "h6") and self.table is None:
            self.heading = {"level": int(tag[1]), "parts": []}
        if tag == "table":
            if self.table is not None:
                raise ValueError("nested table is ambiguous")
            self.table = {"ordinal": len(self.tables) + 1, "line": self.getpos()[0],
                          "attrs": attrs, "headings": dict(self.headings), "rows": []}
        elif tag == "tr" and self.table is not None:
            if self.row is not None:
                raise ValueError("unclosed table row")
            self.row = {"line": self.getpos()[0], "attrs": attrs, "cells": []}
        elif tag in ("td", "th") and self.table is not None:
            if self.row is None or self.cell is not None:
                raise ValueError("cell outside row or unclosed cell")
            if any(attrs.get(key, "1") != "1" for key in ("rowspan", "colspan")):
                raise ValueError("spanning cells require a source-specific adapter")
            self.cell = {"tag": tag, "attrs": attrs, "parts": [], "links": []}
        elif tag == "a" and self.cell is not None and "href" in attrs:
            self.cell["links"].append({"href_as_printed": attrs["href"],
                                       "url": urljoin(self.base_url, attrs["href"])})
        elif tag == "br" and self.cell is not None:
            self.cell["parts"].append(" ")

    def handle_endtag(self, tag):
        if tag in ("script", "style") and self.skip:
            self.skip -= 1
            return
        if self.skip:
            return
        if self.heading is not None and tag == "h" + str(self.heading["level"]):
            level = self.heading["level"]
            self.headings = {k: v for k, v in self.headings.items() if k < level}
            self.headings[level] = clean("".join(self.heading["parts"]))
            self.heading = None
        if tag in ("td", "th") and self.table is not None:
            if self.cell is None or self.cell["tag"] != tag:
                raise ValueError("mismatched cell close")
            self.cell["text"] = clean("".join(self.cell.pop("parts")))
            self.row["cells"].append(self.cell)
            self.cell = None
        elif tag == "tr" and self.table is not None:
            if self.row is None or self.cell is not None:
                raise ValueError("incomplete table row")
            self.table["rows"].append(self.row)
            self.row = None
        elif tag == "table":
            if self.table is None or self.row is not None:
                raise ValueError("incomplete table")
            self.tables.append(self.table)
            self.table = None

    def handle_data(self, data):
        if self.skip:
            return
        if self.cell is not None:
            self.cell["parts"].append(data)
        if self.heading is not None:
            self.heading["parts"].append(data)

    def finish(self, html):
        self.feed(html)
        self.close()
        if any(value is not None for value in (self.table, self.row, self.cell, self.heading)):
            raise ValueError("truncated HTML matrix")
        return self.tables


HEADERS = ["Vendor", "Platform", "ASIC", "Architecture", "Release"]


def parse_bisdn(raw, url):
    tables = MatrixHTML(url).finish(raw.decode("utf-8", errors="strict"))
    matching = [t for t in tables if "hcl-tbl" in t["attrs"].get("class", "").split()]
    if len(matching) != 1:
        raise ValueError("expected exactly one BISDN hcl-tbl; unavailable is not empty")
    table = matching[0]
    rows = table["rows"]
    if not rows or [c["text"] for c in rows[0]["cells"]] != HEADERS:
        raise ValueError("BISDN header schema changed")
    if any(c["tag"] != "th" for c in rows[0]["cells"]):
        raise ValueError("BISDN headers must be explicit th cells")
    result = []
    for ordinal, row in enumerate(rows[1:], 1):
        cells = row["cells"]
        if len(cells) != len(HEADERS) or any(not c["text"] for c in cells):
            raise ValueError(f"BISDN row {ordinal} missing a required cell")
        values = dict(zip(HEADERS, (c["text"] for c in cells)))
        if (row["attrs"].get("data-hcl-vendor") != values["Vendor"] or
                row["attrs"].get("data-hcl-platform") != values["Platform"]):
            raise ValueError(f"BISDN row {ordinal} attribute/cell disagreement")
        if cells[1]["tag"] != "th" or cells[1]["attrs"].get("scope") != "row":
            raise ValueError("BISDN platform must be explicit row header")
        links = cells[4]["links"]
        if len(links) != 1:
            raise ValueError("expected one release download link per row")
        link = urlsplit(links[0]["url"])
        if ((link.scheme, link.netloc, link.path) != ("https", "www.bisdn.de", "/en/linux/download/")
                or parse_qs(link.query) != {"platform": [values["Platform"]]}):
            raise ValueError("release download target disagrees with platform")
        result.append({"row_number": ordinal, "html_line": row["line"], "cells": values,
                       "cell_details": cells, "download_url": links[0]["url"],
                       "section_headings_as_printed": table["headings"],
                       "listing_scope": "listed in captured BISDN HCL; no current/retired label inferred"})
    if not result:
        raise ValueError("empty BISDN matrix; review capture rather than replacing source")
    return {"adapter": "bisdn-html-hcl-v1", "url": url, "body_sha256": hashlib.sha256(raw).hexdigest(),
            "table_number": table["ordinal"], "headers": HEADERS, "row_count": len(result), "rows": result}


def verified_capture(root, receipt_name):
    receipt = json.loads((root / receipt_name).read_text())
    if receipt.get("status") != 200:
        raise ValueError("matrix capture is unavailable")
    path = (root / receipt["body_path"]).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError("capture path escapes input directory")
    raw = path.read_bytes()
    if len(raw) != receipt["body_bytes"] or hashlib.sha256(raw).hexdigest() != receipt["body_sha256"]:
        raise ValueError("matrix capture hash/length mismatch")
    return receipt, raw


def candidate(matrix, receipt):
    sid = "bisdn-linux-hcl-2026-10-14"
    coverage = (f"Complete {matrix['row_count']}-row captured BISDN Linux HTML hardware compatibility table; source-scoped "
                "listing records, not hardware identity/alias/manufacturer/deployment assertions. Exact "
                "Vendor/Platform/ASIC/Architecture/Release cells and download URLs retained. Vendor is "
                "a table label, not physical manufacture. No grouped-name or parenthetical alias expansion. "
                "No installed OS, unit fitment, stock, entitlement, current support guarantee, retired status "
                "or recommendation to flash a device is inferred. Download links were not fetched. "
                "Upstream rights retained; local research only.")
    source = {"id": sid, "url": matrix["url"], "tier": "project",
              "license": "Public factual compatibility listing; original upstream rights retained; local research only.",
              "coverage": coverage, "snapshot": receipt["body_path"],
              "original_sha256": matrix["body_sha256"], "captured_at_utc": receipt["finished_utc"]}
    devices = []
    for row in matrix["rows"]:
        cells = row["cells"]
        scope = f"[captured BISDN HCL table {matrix['table_number']}, data row {row['row_number']}, HTML line {row['html_line']}]"
        facts = [["bisdn_hcl_" + name.lower() + "_as_printed", value + " " + scope]
                 for name, value in cells.items()]
        facts += [["bisdn_hcl_download_url", row["download_url"] + " " + scope],
                  ["bisdn_hcl_listing_scope", row["listing_scope"] + " " + scope]]
        devices.append({"source": sid, "key": f"table-{matrix['table_number']}-row-{row['row_number']}",
                        "brand": "BISDN Linux HCL", "model": cells["Platform"],
                        "title": "BISDN Linux HCL listing: " + cells["Platform"],
                        "kind": "network OS compatibility listing (software reference)", "affiliations": [],
                        "identifiers": [["bisdn_hcl_platform", cells["Platform"]]],
                        "hyperscaler": [], "facts": facts})
    return {"sources": [source], "devices": devices}


def novelty(matrix, database):
    """Read-only literal comparisons; matches are leads, never identity merges."""
    database = database.resolve(strict=True)
    before = hashlib.sha256(database.read_bytes()).hexdigest()
    results = []
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("PRAGMA query_only=ON")
        for row in matrix["rows"]:
            model = row["cells"]["Platform"]
            models = connection.execute("SELECT id,brand,model FROM devices WHERE model=? ORDER BY id", (model,)).fetchall()
            identifiers = connection.execute("SELECT DISTINCT d.id,d.brand,d.model,i.kind,i.value FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id WHERE i.value=? ORDER BY d.id,i.kind", (model,)).fetchall()
            folded = connection.execute("SELECT DISTINCT d.id,d.brand,d.model FROM devices d WHERE lower(d.model)=lower(?) AND d.model<>? ORDER BY d.id", (model, model)).fetchall()
            folded_ids = connection.execute("SELECT DISTINCT d.id,d.brand,d.model,i.kind,i.value FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id WHERE lower(i.value)=lower(?) AND i.value<>? ORDER BY d.id,i.kind", (model, model)).fetchall()
            results.append({"row_number": row["row_number"], "platform_as_printed": model,
                            "exact_model_matches": models, "exact_identifier_matches": identifiers,
                            "case_insensitive_only_model_leads": folded,
                            "case_insensitive_only_identifier_leads": folded_ids,
                            "absent_as_exact_model_or_identifier": not models and not identifiers})
    after = hashlib.sha256(database.read_bytes()).hexdigest()
    if before != after:
        raise ValueError("catalog changed during novelty comparison")
    return {"database": str(database), "database_sha256_before": before,
            "database_sha256_after": after, "comparison": "literal exact strings; lowercase-only leads separate; no substring/alias expansion or merge",
            "rows_compared": len(results), "rows_absent_as_exact_model_or_identifier": sum(r["absent_as_exact_model_or_identifier"] for r in results),
            "rows": results}


def dump(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--receipt", default="raw/05-bisdn-hcl.json")
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    receipt, raw = verified_capture(args.root, args.receipt)
    matrix = parse_bisdn(raw, receipt["url"])
    args.output.mkdir(parents=True, exist_ok=True)
    dump(args.output / "bisdn-matrix.json", matrix)
    candidate_receipt = dict(receipt, body_path=os.path.relpath(args.root / receipt["body_path"], args.output))
    dump(args.output / "candidate.json", candidate(matrix, candidate_receipt))
    dump(args.output / "novelty.json", novelty(matrix, args.database))
    print(json.dumps({"rows": matrix["row_count"], "candidate_sources": 1,
                      "candidate_records": matrix["row_count"], "candidate_facts": matrix["row_count"] * 7}))


if __name__ == "__main__":
    main()
