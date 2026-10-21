"""Guarded local wording-only PVF installation. Author: zkonikishi."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(os.environ['CLIENT_ROOT'])
HERE = Path(__file__).resolve().parent
CANDIDATE = Path(os.environ['CANDIDATE_PVF'])
OLD = os.environ['EXPECTED_SOURCE_SHA256']
NEW = os.environ['EXPECTED_TARGET_SHA256']


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def stopped():
    result = subprocess.run(['powershell', '-NoProfile', '-Command',
        'if (Get-Process DFO -ErrorAction SilentlyContinue) {exit 9}'], capture_output=True)
    assert result.returncode == 0, 'Game running or process check failed'


def main():
    approval = json.loads((HERE / 'compatibility.json').read_text(encoding='utf-8-sig'))
    assert approval['approved_candidate_sha256'] == NEW
    assert approval['launcher_identity_compatible']
    assert approval['deployment_mode'] == 'reversible_test_client_install'
    stopped()
    target = ROOT / 'Script.pvf'
    registry = ROOT / '.launcher-mods/modkit/registry.json'
    raw = registry.read_bytes()
    assert raw == (HERE / 'registry.before.json').read_bytes(), 'Concurrent registry change'
    assert sha(target) == OLD and sha(HERE / 'before-Script.pvf') == OLD
    assert sha(CANDIDATE) == NEW
    # Preserve every unrelated registry byte and the original uninstall backup.
    text = raw.decode('utf-8')
    needle = '"installedSHA256": "' + OLD + '"'
    assert text.count(needle) == 1
    updated = text.replace(needle, '"installedSHA256": "' + NEW + '"')
    json.loads(updated)
    stage = ROOT / 'Script.pvf.localization17-staged'
    assert not stage.exists(), 'Unresolved staged transaction'
    shutil.copy2(CANDIDATE, stage)
    assert sha(stage) == NEW
    stopped()
    assert registry.read_bytes() == raw and sha(target) == OLD
    journal = {'author': 'zkonikishi', 'client': str(ROOT), 'status': 'committing',
               'before': OLD, 'after': NEW, 'text_changes': 17,
               'sk_dat_unchanged': True, 'game_started': False, 'runtime_verified': False,
               'compatibility': approval,
               'registry_policy': 'Preserve original owner/uninstall backup; update installed digest only',
               'rollback_pvf': str(HERE / 'before-Script.pvf')}
    receipt = HERE / 'install-receipt.json'
    receipt.write_text(json.dumps(journal, indent=2), encoding='utf-8')
    try:
        os.replace(stage, target)
        assert sha(target) == NEW
        assert registry.read_bytes() == raw, 'Concurrent registry write after PVF commit'
        regstage = registry.with_suffix('.localization17-staged')
        regstage.write_bytes(updated.encode('utf-8'))
        os.replace(regstage, registry)
        assert sha(ROOT / 'sk.dat') == sha(HERE / 'before-sk.dat')
        journal['status'] = 'committed'
    except Exception:
        # Do not clobber a concurrent owner's registry edits during compensation.
        if sha(target) == NEW:
            shutil.copy2(HERE / 'before-Script.pvf', stage)
            os.replace(stage, target)
        if registry.read_bytes() == updated.encode('utf-8'):
            registry.write_bytes(raw)
        journal['status'] = 'compensated'
        raise
    finally:
        receipt.write_text(json.dumps(journal, indent=2), encoding='utf-8')
    print(json.dumps(journal))


if __name__ == '__main__':
    main()
