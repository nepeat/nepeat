"""Bounded offline Poppler extraction helper; staged output, no imports."""
import datetime
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE_ROOT = ROOT.parent / "psu"
TOOL_ROOT = Path("/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin")
FILES = (
    "bel-pfe1100-series.pdf",
    "bel-pfe1500-series.pdf",
    "bel-pfe1500-nemko.pdf",
    "bel-pfe1500-csa.pdf",
    "fsp550-29fm.pdf",
)
METHOD = "poppler pdftotext -layout embedded text; no OCR"


def sha(body):
    return hashlib.sha256(body).hexdigest()


def run(argv, output_prefix):
    result = subprocess.run(argv, capture_output=True, timeout=30)
    (ROOT / (output_prefix + ".stdout")).write_bytes(result.stdout)
    (ROOT / (output_prefix + ".stderr")).write_bytes(result.stderr)
    command = {
        "argv": argv,
        "returncode": result.returncode,
        "stdout_file": output_prefix + ".stdout",
        "stderr_file": output_prefix + ".stderr",
        "stdout_sha256": sha(result.stdout),
        "stderr_sha256": sha(result.stderr),
    }
    return result, command


def main():
    captures = json.loads((SOURCE_ROOT / "fetch-report.json").read_text())
    expected = {row["file"]: row["sha256"] for row in captures if row.get("outcome") == "captured"}
    report = {
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "extraction_method": METHOD,
        "input_limit": 5,
        "production_import_support": False,
        "commands": [],
        "inputs": [],
    }
    version_result, command = run([str(TOOL_ROOT / "pdftotext"), "-v"], "pdftotext-version")
    report["commands"].append(command)
    version_text = (version_result.stdout + version_result.stderr).decode("utf-8", errors="strict")
    match = re.search(r"pdftotext version (\S+)", version_text)
    if version_result.returncode != 0 or match is None or match.group(1) != "26.06.0":
        raise RuntimeError("Unexpected Poppler version/status: " + version_text)
    report["extractor_version"] = match.group(1)
    for filename in FILES:
        source = SOURCE_ROOT / filename
        entry = {"source": str(source), "source_file": filename, "commands": []}
        stem = source.stem
        try:
            body = source.read_bytes()
            entry["source_sha256_before"] = sha(body)
            if not body.startswith(b"%PDF-") or sha(body) != expected[filename]:
                raise ValueError("Source header or recorded capture hash mismatch")
            result, command = run([str(TOOL_ROOT / "pdfinfo"), str(source)], stem + "-pdfinfo")
            entry["commands"].append(command)
            if result.returncode != 0:
                raise ValueError("pdfinfo failed")
            info = result.stdout.decode("utf-8", errors="strict")
            page_match = re.search(r"^Pages:\s+(\d+)\s*$", info, re.MULTILINE)
            if page_match is None or int(page_match.group(1)) < 1:
                raise ValueError("Missing or invalid pdfinfo page count")
            count = int(page_match.group(1))
            entry["page_count"] = count
            pages = []
            for number in range(1, count + 1):
                argv = [
                    str(TOOL_ROOT / "pdftotext"),
                    "-f", str(number), "-l", str(number),
                    "-layout", "-enc", "UTF-8", str(source), "-",
                ]
                result, command = run(argv, stem + "-page-" + str(number))
                entry["commands"].append(command)
                if result.returncode != 0:
                    raise ValueError("pdftotext failed on page " + str(number))
                text = result.stdout.decode("utf-8", errors="strict")
                removed_final_delimiter = text.endswith("\f")
                if removed_final_delimiter:
                    text = text[:-1]
                command["removed_one_final_form_feed"] = removed_final_delimiter
                if "\f" in text:
                    raise ValueError("Extra/nonfinal form-feed delimiter on page " + str(number))
                pages.append({"page": number, "text": text})
            entry["source_sha256_after"] = sha(source.read_bytes())
            if entry["source_sha256_after"] != entry["source_sha256_before"]:
                raise ValueError("Original source changed during extraction")
            if [p["page"] for p in pages] != list(range(1, count + 1)):
                raise ValueError("Missing or reordered extraction pages")
            envelope = {
                "pdf_sha256": entry["source_sha256_before"],
                "extraction_method": METHOD,
                "extractor_version": report["extractor_version"],
                "page_count": count,
                "pages": pages,
            }
            output = ROOT / (stem + "-text.json")
            output.write_text(json.dumps(envelope, ensure_ascii=False, indent=2) + "\n")
            parsed = json.loads(output.read_text())
            if len(parsed["pages"]) != count or parsed["pdf_sha256"] != expected[filename]:
                raise ValueError("Saved envelope verification failed")
            entry.update(
                outcome="extracted",
                envelope_file=output.name,
                envelope_sha256=sha(output.read_bytes()),
                blank_text_pages=[p["page"] for p in pages if not p["text"].strip()],
                pages_with_stderr=[n for n, c in enumerate(entry["commands"][1:], 1)
                                   if (ROOT / c["stderr_file"]).stat().st_size],
            )
        except Exception as error:
            entry.update(outcome="failed", error=repr(error))
        report["inputs"].append(entry)
    report["completed_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["artifacts"] = [
        {"file": p.name, "bytes": p.stat().st_size, "sha256": sha(p.read_bytes())}
        for p in sorted(ROOT.iterdir()) if p.is_file() and p.name != "extraction-report.json"
    ]
    (ROOT / "extraction-report.json").write_text(json.dumps(report, indent=2) + "\n")
    for entry in report["inputs"]:
        print(entry["source_file"], entry["outcome"], entry.get("page_count"),
              entry.get("blank_text_pages"), entry.get("error", ""))
    if any(entry["outcome"] != "extracted" for entry in report["inputs"]):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
