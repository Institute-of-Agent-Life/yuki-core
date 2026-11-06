"""Verify three local suites for the current GitHub PR merge checkout."""
import argparse
import json
from pathlib import Path
import subprocess

from source import digest, source_identity


ROOT = Path(__file__).resolve().parents[1]
REPO = 'Institute-of-Agent-Life/yuki-core'
PLATFORMS = {'macos': 'macOS-', 'linux': 'Linux-', 'windows': 'Windows-'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_suite(directory, platform, source, registry):
    suite = json.loads((directory / 'suite.json').read_bytes())
    cards = json.loads(registry)['cards']
    require(suite['verified'] is True and suite['registered'] == len(cards)
            and set(suite['cards']) == set(cards) and not suite['failed']
            and not suite['not_finished'], f'{platform}: incomplete suite')
    manifests = {}
    for card, result in suite['cards'].items():
        folder = directory / card / 'ci'
        raw = (folder / 'manifest.json').read_bytes()
        m = json.loads(raw)
        require(m['card'] == card and m['platform'].startswith(PLATFORMS[platform])
                and m['source']['commit'] == source['commit'] and not m['source']['dirty']
                and m['source']['tree_sha256'] == source['tree_sha256']
                and m['source']['files'] == source['files']
                and m['lock_sha256'] == digest(registry), f'{platform}/{card}: wrong source or platform')
        require(result['exit_code'] == m['exit_code'] == 0 and m['status'] == 'verified'
                and not m['failure'] and m['assertions'] and all(a['passed'] is True for a in m['assertions'])
                and cards[card]['artifact'] in m['artifacts'], f'{platform}/{card}: unverified card')
        for name, sha in m['artifacts'].items():
            path = (folder / name).resolve()
            require(path.is_relative_to(folder.resolve()) and path.is_file()
                    and digest(path.read_bytes()) == sha, f'{platform}/{card}: invalid evidence {name}')
        manifests[card] = digest(raw)
    return {'platform': platform, 'cards': len(cards), 'seconds': suite['seconds'],
            'manifest_sha256': manifests, 'suite_sha256': digest((directory / 'suite.json').read_bytes())}


def api(endpoint, *args):
    return json.loads(subprocess.check_output(['gh', 'api', f'repos/{REPO}/{endpoint}', *args]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pr', type=int)
    parser.add_argument('--checkout', type=Path, default=ROOT)
    parser.add_argument('--out', required=True, type=Path)
    for platform in PLATFORMS:
        parser.add_argument('--' + platform, required=True, type=Path)
    parser.add_argument('--publish', action='store_true', help='write local CI statuses to the PR')
    parser.add_argument('--merge', action='store_true', help='publish and merge a verified main PR')
    args = parser.parse_args()
    checkout = args.checkout.resolve()
    require(not (checkout / '.yuki-source.json').exists(), 'verification requires a clean Git checkout')
    source = source_identity(checkout)
    require(not source['dirty'], 'verification requires a clean Git checkout')
    parents = subprocess.check_output(['git', '-C', str(checkout), 'show', '-s', '--format=%P',
                                       source['commit']], text=True).split()
    require(len(parents) == 2, 'expected the PR merge commit')

    def current():
        pr = api(f'pulls/{args.pr}')
        require(pr['state'] == 'open' and pr['head']['repo']['full_name'] == REPO
                and [pr['base']['sha'], pr['head']['sha']] == parents
                and pr['merge_commit_sha'] == source['commit']
                and api('git/ref/heads/' + pr['base']['ref'])['object']['sha'] == parents[0],
                'PR head, base or prospective merge changed')
        if args.merge:
            require(pr['base']['ref'] == 'main', 'merge requires a main PR')
        return pr

    current()
    registry = (checkout / 'ci/checks.json').read_bytes()
    results = [verify_suite(getattr(args, platform), platform, source, registry) for platform in PLATFORMS]
    proof = {'pr': args.pr, 'base': parents[0], 'head': parents[1], 'merge': source['commit'],
             'tree_sha256': source['tree_sha256'], 'platforms': results}
    data = json.dumps(proof, indent=2).encode() + b'\n'
    if args.out.exists():
        require(args.out.read_bytes() == data, 'receipt already exists for a different run')
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        with args.out.open('xb') as f:
            f.write(data)
    if args.publish or args.merge:
        current()
        for result in results:
            context = 'local-ci/' + result['platform']
            description = f"{result['cards']} cards, merge {source['commit'][:12]}, receipt {digest(data)[:12]}"
            status = api('statuses/' + parents[1], '--method', 'POST', '-f', 'state=success',
                         '-f', 'context=' + context, '-f', 'description=' + description)
            require(status['state'] == 'success' and status['context'] == context, 'status write failed')
    if args.merge:
        current()
        subprocess.run(['gh', 'pr', 'merge', str(args.pr), '--repo', REPO, '--merge',
                        '--match-head-commit', parents[1]], check=True)
    print(args.out, digest(data))


if __name__ == '__main__':
    main()
