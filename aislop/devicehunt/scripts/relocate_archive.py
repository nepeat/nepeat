"""Rebind the operational archive authority after moving this entire workspace.

Run from the destination with PYTHONPATH=. after the move, once nothing else
uses the archive. Only the path-bound archive identity changes (operational_meta,
capture-authority.json, capture-authority-ready.json); quota, lease, event and
receipt rows are kept, and a verified backup of the operational store is written
first. The old root must no longer exist: a copy is never given the authority.
Repeating the same command finishes an interrupted relocation.
"""
import argparse
import json
from pathlib import Path

from devicehunt.artifacts import Archive
from devicehunt.campaign_state import CampaignState


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--from-root', required=True, help='old absolute workspace root')
    parser.add_argument('--to-root', default='.')
    parser.add_argument('--decision', required=True, help='recorded reason for the move')
    parser.add_argument('--quiesced', action='store_true', required=True)
    args = parser.parse_args()
    old, new = Path(args.from_root).absolute(), Path(args.to_root).absolute()
    state = CampaignState.relocate(new / 'data/archive', old_root=old / 'data/archive',
                                   quiesced=args.quiesced, decision=args.decision)
    archive = Archive(new / 'data/archive', new, readonly=True)
    try:
        ready = archive.require_ready()
    finally:
        archive.close()
    with state.db() as db:
        meta = dict(db.execute('SELECT archive_id,authority_id,recovery FROM operational_meta').fetchone())
    print(json.dumps(dict(from_archive=str(old / 'data/archive'), to_archive=str(state.root),
                          operational=meta, archive_sequence=ready['sequence']), indent=2))


if __name__ == '__main__':
    main()
