#!/usr/bin/env bash
# Hardware-free integration only: no Bluetooth radio, network, ADB or system keys.
set -euo pipefail
console_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$console_root"
cargo build --manifest-path src-tauri/Cargo.toml --example console_ble_simulation
(
  cd mobile/android
  ./gradlew :repose_unlock_native:testDebugUnitTest \
    --tests '*ConsoleBleSimulationTest' --rerun-tasks
)
python3 - "$console_root" <<'PY'
import pathlib
import sys
import xml.etree.ElementTree as ET
report = pathlib.Path(sys.argv[1]) / 'mobile/build/repose_unlock_native/test-results/testDebugUnitTest/TEST-ai.repose.mobile.unlock.console.ConsoleBleSimulationTest.xml'
root = ET.parse(report).getroot()
assert int(root.attrib['tests']) > 0, 'No integration tests executed'
for field in ('failures', 'errors', 'skipped'):
    assert int(root.attrib.get(field, '0')) == 0, f'Integration report has {field}'
print('Kotlin/Rust Bluetooth simulation passed with zero skipped tests.')
PY
