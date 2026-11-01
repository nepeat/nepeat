#!/usr/bin/env python3
"""Extract a new, source-bound local PDF sidecar using explicit Poppler tools."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from devicehunt.documents import POPPLER_METHOD, POPPLER_OPTIONS, read_capture

TIMEOUT_SECONDS = 30


def sha(body):
    return hashlib.sha256(body).hexdigest()


def extract(pdf, output, tool_directory):
    pdf = Path(pdf).resolve()
    output = Path(output).absolute()
    tools = Path(tool_directory).resolve()
    if output.exists() or output.is_symlink():
        raise ValueError('Output already exists; choose a new capture path')
    if not output.parent.is_dir():
        raise ValueError('Output parent directory must already exist')
    executables = {name: tools / name for name in ('pdfinfo', 'pdftotext')}
    for path in executables.values():
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ValueError('Missing executable local Poppler tool: ' + str(path))
    before = pdf.read_bytes()
    if not before.startswith(b'%PDF-'):
        raise ValueError('Input is not a PDF')
    pdf_hash = sha(before)
    commands = []
    env = dict(os.environ, LC_ALL='C', LANG='C')

    def run(name, args):
        argv = [str(executables[name])] + args
        result = subprocess.run(argv, capture_output=True, timeout=TIMEOUT_SECONDS,
                                env=env, shell=False)
        # Decode strictly and preserve stderr, including successful warnings.
        stdout = result.stdout.decode('utf-8')
        stderr = result.stderr.decode('utf-8')
        command = {'argv': argv, 'returncode': result.returncode,
                   'stderr': stderr, 'stdout_sha256': sha(result.stdout)}
        commands.append(command)
        if result.returncode != 0:
            raise ValueError(name + ' failed: ' + stderr)
        return stdout, command

    versions = {}
    version_outputs = {}
    for name in ('pdftotext', 'pdfinfo'):
        stdout, command = run(name, ['-v'])
        raw = stdout + command['stderr']
        match = re.search(re.escape(name) + r' version (\d+\.\d+(?:\.\d+)?)\b', raw)
        if match is None:
            raise ValueError('Unrecognized ' + name + ' version')
        versions[name] = match.group(1)
        version_outputs[name] = {'stdout': stdout, 'stderr': command['stderr']}
    if versions['pdftotext'] != versions['pdfinfo']:
        raise ValueError('Poppler tool versions must agree')
    info, _ = run('pdfinfo', [str(pdf)])
    counts = re.findall(r'^Pages:\s*([0-9]+)\s*$', info, re.MULTILINE)
    encrypted = re.findall(r'^Encrypted:\s*(.+)$', info, re.MULTILINE)
    if len(counts) != 1 or int(counts[0]) < 1:
        raise ValueError('Invalid pdfinfo page count')
    if len(encrypted) != 1 or encrypted[0].strip() != 'no':
        raise ValueError('Encrypted/password or unknown encryption status is unsupported')
    count = int(counts[0])
    pages = []
    for ordinal in range(1, count + 1):
        text, _ = run('pdftotext', [
            '-f', str(ordinal), '-l', str(ordinal), '-layout', '-enc', 'UTF-8',
            '-eol', 'unix', '-remove-hyphens', 'none', '-nopgbrk', str(pdf), '-',
        ])
        if '\f' in text:
            raise ValueError('Unexpected page delimiter with -nopgbrk')
        pages.append({'page': ordinal, 'text': text})
    if sha(pdf.read_bytes()) != pdf_hash:
        raise ValueError('PDF changed during extraction')
    capture = {
        'pdf_sha256': pdf_hash, 'extraction_method': POPPLER_METHOD,
        'extractor_version': versions['pdftotext'], 'pdfinfo_version': versions['pdfinfo'],
        'pdf_page_count': count, 'extraction_options': dict(POPPLER_OPTIONS),
        'pages': pages,
        'provenance': {
            'source_filename': pdf.name,
            'extracted_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'tool_directory': str(tools), 'pdfinfo_stdout': info,
            'version_outputs': version_outputs, 'commands': commands,
            'timeout_seconds': TIMEOUT_SECONDS, 'locale': 'C',
        },
    }
    serialized = (json.dumps(capture, ensure_ascii=False, indent=2) + '\n').encode('utf-8')
    read_capture(serialized, pdf_hash)
    # A same-directory temporary file and exclusive link publish the complete
    # capture atomically without overwriting a concurrently created output.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=output.parent, prefix='.pdf-capture-', delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(serialized)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, output)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return capture


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pdf', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--tool-directory', required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        capture = extract(args.pdf, args.output, args.tool_directory)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, str(error) + '\n')
    print(str(args.output) + ': ' + str(capture['pdf_page_count']) + ' pages')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
