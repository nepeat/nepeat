"""Fetch public detail URLs from an RRA CSV's final certificate-number column.

Other CSV columns can contain unquoted commas and are deliberately not parsed.
Uses the URL pattern documented at https://www.rra.go.kr/ko/notice/go_7_4.do.
No cookies, credentials, alternate access routes or automatic retries.
"""
import csv
import hashlib
import io
import json
import re
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote

HEADER = ['번호', '상호', '기자재명칭', '모델명', '제조국가', '인증 연월일', '인증 상태', '인증번호']
BASE = 'https://www.rra.go.kr/ko/license/A_b_popup_keyno.do?key_no='


def certificates(body):
    rows = [r for r in csv.reader(io.StringIO(body.decode('cp949')), strict=True) if r]
    if not rows or [c.strip() for c in rows[0]] != HEADER:
        raise ValueError('RRA CSV header differs')
    numbers = []
    for n, row in enumerate(rows[1:], 1):
        if (len(row) < 8 or row[0] != str(n)
                or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9/_-]+', row[-1])):
            raise ValueError('Unrecognized RRA CSV row ' + str(n))
        numbers.append(row[-1])
    if not numbers or len(set(numbers)) != len(numbers):
        raise ValueError('Empty or duplicated RRA certificate list')
    return numbers


def main():
    source, out = map(Path, sys.argv[1:])
    out.mkdir(parents=True, exist_ok=True)
    numbers = certificates(source.read_bytes())
    entries = []
    for n, certificate in enumerate(numbers, 1):
        url = BASE + quote(certificate, safe='')
        name = 'detail-' + str(n).zfill(3) + '.html'
        file = out / name
        if file.exists():
            raise ValueError('Use a new output directory; refusing to overwrite ' + str(file))
        subprocess.run(['curl', '--silent', '--show-error', '--fail', '--location',
                        '--max-time', '45', url, '--output', str(file)], check=True)
        entries.append(dict(certificate=certificate, url=url, path=name,
                            sha256=hashlib.sha256(file.read_bytes()).hexdigest()))
        (out / 'capture-progress.json').write_text(json.dumps(dict(complete=False, entries=entries), indent=2)+'\n')
        print(n, '/', len(numbers), certificate, flush=True)
        time.sleep(0.15)
    (out / 'capture-progress.json').write_text(json.dumps(dict(complete=True, entries=entries), indent=2)+'\n')


if __name__ == '__main__':
    main()
