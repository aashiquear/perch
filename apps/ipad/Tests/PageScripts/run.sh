#!/usr/bin/env bash
# Run the scripts the glasses windows inject into pages (Sources/Perch/Host/
# HostScripts.swift) against a small test page in headless Chrome and check
# what they did. Chrome is not WebKit, so this proves the logic, not the iPad.
#   usage: Tests/PageScripts/run.sh        (from apps/ipad; needs google-chrome and python3)
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT="$(mktemp -d)"
python3 - "$OUT" <<'PY'
import json, re, sys
out = sys.argv[1]
s = open('Sources/Perch/Host/HostScripts.swift').read()
js = {n: b.replace('\\\\', '\\') for n, b in re.findall(r'static let (\w+) = """\n(.*?)\n    """', s, re.S)}
js['gamepad'] = re.search(r'static let gamepad = "(.*)"', s).group(1)
html = open('Tests/PageScripts/pagetest.tmpl').read()
for k in ['inputShim', 'standaloneShim', 'fullscreenShim', 'gamepadShim']:
    html = html.replace('/*%s*/' % k, js[k])
for k in ['click', 'scroll', 'gamepad']:
    html = html.replace('"%%%s%%"' % k, json.dumps(js[k]))
open(out + '/pagetest.html', 'w').write(html)
PY
CHROME="$(command -v google-chrome || command -v chromium || command -v chromium-browser)"
"$CHROME" --headless=new --no-sandbox --disable-gpu --virtual-time-budget=3000 --dump-dom "file://$OUT/pagetest.html" 2>/dev/null \
  | grep -o 'RESULT{.*}END' | tail -1 | sed 's/^RESULT//; s/END$//' > "$OUT/result.json"
python3 - "$OUT/result.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
want = {
    'svgClick': 1,                 # an icon drawn as SVG is clickable
    'touchend': 1,                 # touch-only handlers get the tap
    'hash': '#went',               # links navigate
    'shadowClick': True,           # reaches into shadow roots
    'checked': True,               # form controls toggle
    'standalone': True, 'mm': True, 'mmOther': False,
    'fsEnabled': True, 'fsSyncEvents': 0, 'fsFills': [True, True], 'fsEl': 'fs', 'fsEvents': ['fs', None], 'fsAfter': 50,
    'pad': [1, 'standard', 0.5, True, 0.7, 17],
}
bad = {k: (r.get(k), v) for k, v in want.items() if r.get(k) != v}
if 'clickAfterPrevented' in r: bad['clickAfterPrevented'] = (True, 'absent')
if 'exception' in r: bad['exception'] = (r['exception'], 'none')
if bad:
    print('FAILED'); [print(' ', k, 'got', g, 'want', w) for k, (g, w) in bad.items()]; sys.exit(1)
print('page scripts: %d checks passed' % len(want))
PY
