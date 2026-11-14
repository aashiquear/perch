# Live-site harness: load a real page in WebKitGTK with the scripts the glasses
# windows inject (HostScripts.swift), run JS steps, print the page log.
#   python3 Tests/PageScripts/wk.py <url> <steps.json>      (opens a muted window)
# steps: [{"name":..,"js":"...return x"} | {"script":"click","args":{"x":1,"y":2}}, "wait": seconds]
# WK_PRE=<file.js> injects an extra script first (e.g. to imitate an iPad).
import gi, json, re, sys
gi.require_version('Gtk', '3.0'); gi.require_version('WebKit2', '4.1')
from gi.repository import Gtk, WebKit2, GLib
src = open(__import__('os').path.join(__import__('os').path.dirname(__import__('os').path.abspath(__file__)), '../../Sources/Perch/Host/HostScripts.swift')).read()
JS = {n: b.replace('\\\\', '\\') for n, b in re.findall(r'static let (\w+) = """\n(.*?)\n    """', src, re.S)}
url, steps = sys.argv[1], json.load(open(sys.argv[2]))
ucm = WebKit2.UserContentManager()
for k in ['inputShim', 'standaloneShim', 'fullscreenShim', 'gamepadShim']:
    ucm.add_script(WebKit2.UserScript(JS[k], WebKit2.UserContentInjectedFrames.ALL_FRAMES, WebKit2.UserScriptInjectionTime.START, None, None))
import os
EXTRA=os.environ.get('WK_PRE')
if EXTRA: ucm.add_script(WebKit2.UserScript(open(EXTRA).read(), WebKit2.UserContentInjectedFrames.ALL_FRAMES, WebKit2.UserScriptInjectionTime.START, None, None))
ucm.register_script_message_handler('perch')
ucm.connect('script-message-received::perch', lambda m, r: print('LOG', r.get_js_value().to_string(), flush=True))
view = WebKit2.WebView.new_with_user_content_manager(ucm)
st = view.get_settings()
st.set_user_agent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15')
st.set_media_playback_requires_user_gesture(False); st.set_enable_developer_extras(True)
view.set_is_muted(True)
win = Gtk.Window(); win.set_default_size(1440, 810); win.add(view); win.show_all()
def run(i=0):
    if i >= len(steps): Gtk.main_quit(); return False
    s = steps[i]
    body = JS[s['script']] if 'script' in s else s['js']
    args = s.get('args', {})
    code = 'window.__r="pending";(async function(%s){%s})(%s).then(function(v){window.__r=String(v)},function(e){window.__r="EXC "+e});0' % (','.join(args.keys()), body, ','.join(json.dumps(v) for v in args.values()))
    def read():
        def got(v, res, _):
            try: print('STEP', i, s.get('name', ''), '->', v.evaluate_javascript_finish(res).to_string(), flush=True)
            except Exception as e: print('STEP', i, 'ERR', e, flush=True)
            run(i + 1)
        view.evaluate_javascript('window.__r', -1, None, None, None, got, None)
        return False
    view.evaluate_javascript(code, -1, None, None, None, lambda v, res, _: GLib.timeout_add(int(s.get('wait', 1) * 1000), read), None)
    return False
view.connect('load-changed', lambda v, e: e == WebKit2.LoadEvent.FINISHED and GLib.timeout_add(6000, run) and None)
view.load_uri(url)
GLib.timeout_add(90000, Gtk.main_quit)
Gtk.main()
