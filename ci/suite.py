"""Run every registered card on one native checkout. Only case processes overlap."""
import argparse
from concurrent.futures import ThreadPoolExecutor, wait, FIRST_COMPLETED
import json
from pathlib import Path
import sys
import time

from check import ROOT, run_card


def run(out):
    out.mkdir(parents=True, exist_ok=False)
    cards = json.loads((ROOT / 'ci/checks.json').read_bytes())['cards']
    if not cards:
        raise ValueError('empty registry')
    results = {}
    started = time.monotonic()

    def one(card, prepared=None):
        print('Starting ' + card, flush=True)
        clock = time.monotonic()
        rc = run_card(argparse.Namespace(card=card, out=out / card / 'ci', fault=None), prepared)
        return {'exit_code': rc, 'seconds': round(time.monotonic() - clock, 3)}

    def save():
        pending = sorted(cards.keys() - results.keys())
        failed = [c for c, r in results.items() if r['exit_code']]
        ok = not pending and not failed
        summary = {'verified': ok, 'failed': failed, 'not_finished': pending, 'cards': results,
                   'registered': len(cards), 'workers': 2, 'seconds': round(time.monotonic() - started, 3)}
        (out / 'suite.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
        return ok

    # Build both profiles before readers start, and retain the original stage logs in every card.
    anchor = next((c for c, s in cards.items() if s.get('release')), None)
    if anchor is None:
        anchor = next((c for c, s in cards.items() if s.get('rust')), None)
    prepared = None
    save()
    if anchor:
        results[anchor] = one(anchor)
        save()
        if results[anchor]['exit_code']:
            return 1
        path = out / anchor / 'ci'
        receipt = json.loads((path / 'manifest.json').read_bytes())
        if receipt['status'] != 'verified' or receipt['card'] != anchor:
            raise ValueError('no verified native build')
        prepared = path, receipt

    # Start the slowest observed Windows cases first; new cards are always included.
    heavy = ['CORE-051', 'CORE-050', 'CORE-034', 'CORE-037', 'CORE-058', 'CORE-035', 'CORE-023']
    queue = iter(sorted(cards.keys() - results.keys(), key=lambda c: (heavy.index(c) if c in heavy else len(heavy), c)))
    failed = False
    with ThreadPoolExecutor(max_workers=2) as pool:
        active = {}
        while True:
            while not failed and len(active) < 2 and (card := next(queue, None)) is not None:
                active[pool.submit(one, card, prepared)] = card
            if not active:
                break
            done, _ = wait(active, return_when=FIRST_COMPLETED)
            for future in done:
                card = active.pop(future)
                results[card] = future.result()
                failed |= bool(results[card]['exit_code'])
            save()
    return 0 if save() else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True, type=Path)
    sys.exit(run(parser.parse_args().out.resolve()))
