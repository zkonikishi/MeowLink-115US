"""Revert only this wording layer, retaining later unrelated registry edits."""
import json
import os
import shutil
from install import ROOT, HERE, OLD, NEW, sha, stopped

stopped()
receipt = json.loads((HERE / 'install-receipt.json').read_text(encoding='utf-8'))
assert receipt['status'] == 'committed', 'No committed install to revert'
target = ROOT / 'Script.pvf'
registry = ROOT / '.launcher-mods/modkit/registry.json'
raw = registry.read_bytes()
text = raw.decode('utf-8')
needle = '"installedSHA256": "' + NEW + '"'
assert text.count(needle) == 1, 'Registry no longer owns this exact version'
assert sha(target) == NEW and sha(HERE / 'before-Script.pvf') == OLD
stage = ROOT / 'Script.pvf.localization17-rollback'
assert not stage.exists(), 'Unresolved rollback staging file'
shutil.copy2(HERE / 'before-Script.pvf', stage)
assert sha(stage) == OLD
stopped()
assert registry.read_bytes() == raw and sha(target) == NEW
os.replace(stage, target)
updated = text.replace(needle, '"installedSHA256": "' + OLD + '"')
regstage = registry.with_suffix('.localization17-rollback')
regstage.write_bytes(updated.encode('utf-8'))
assert registry.read_bytes() == raw, 'Concurrent registry write; manual reconciliation required'
os.replace(regstage, registry)
(HERE / 'rollback-receipt.json').write_text(json.dumps({
    'author': 'zkonikishi', 'status': 'restored', 'pvf_sha256': sha(target),
    'unrelated_registry_edits_preserved': True, 'game_started': False,
}, indent=2), encoding='utf-8')
print('Restored original wording PVF; unrelated modifications preserved')
