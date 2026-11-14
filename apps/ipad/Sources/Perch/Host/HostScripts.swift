import Foundation

/// The page-side half of the touchpad: there is no public way to synthesise
/// touches into another window, so pointer and keyboard actions are performed
/// by script at the cursor position.
enum Script {
    static let hover = """
    var at = window.__perchAt ? window.__perchAt(x, y) : null; if (!at) return;
    var o = {bubbles: true, cancelable: true, composed: true, view: at.win, clientX: at.x, clientY: at.y, pointerId: 1, pointerType: "mouse", isPrimary: true};
    at.el.dispatchEvent(new PointerEvent("pointermove", o)); at.el.dispatchEvent(new MouseEvent("mousemove", o));
    at.el.dispatchEvent(new MouseEvent("mouseover", o));
    """

    /// A tap, as an iPad delivers one: pointer and touch events, then the
    /// mouse events and the click. Events are dispatched (never `el.click()`,
    /// which icons drawn as SVG do not have), reach into shadow roots and
    /// same-origin frames, and stop after `touchend` if the page says it has
    /// handled the tap itself.
    static let click = """
    var at = window.__perchAt ? window.__perchAt(x, y) : null; if (!at) return "none";
    var e = at.el, w = at.win, cx = at.x, cy = at.y;
    var base = {bubbles: true, cancelable: true, composed: true, view: w, clientX: cx, clientY: cy, screenX: cx, screenY: cy, button: 0, detail: 1};
    function P(type, buttons) { return e.dispatchEvent(new w.PointerEvent(type, Object.assign({pointerId: 1, pointerType: "touch", isPrimary: true, buttons: buttons, pressure: buttons ? 0.5 : 0, width: 20, height: 20}, base))); }
    function M(type, buttons) { return e.dispatchEvent(new w.MouseEvent(type, Object.assign({buttons: buttons}, base))); }
    var handled = false;
    P("pointerdown", 1);
    try {
      var t = new w.Touch({identifier: 1, target: e, clientX: cx, clientY: cy, pageX: cx + w.scrollX, pageY: cy + w.scrollY, screenX: cx, screenY: cy, radiusX: 10, radiusY: 10, force: 0.5});
      var tb = {bubbles: true, cancelable: true, composed: true, view: w};
      e.dispatchEvent(new w.TouchEvent("touchstart", Object.assign({touches: [t], targetTouches: [t], changedTouches: [t]}, tb)));
      P("pointerup", 0);
      handled = !e.dispatchEvent(new w.TouchEvent("touchend", Object.assign({touches: [], targetTouches: [], changedTouches: [t]}, tb)));
    } catch (err) { P("pointerup", 0); }
    var f = e.closest ? e.closest("input,textarea,select,[contenteditable=''],[contenteditable='true']") : null;
    if (f) f.focus(); else if (w.document.activeElement && w.document.activeElement !== w.document.body && !handled) w.document.activeElement.blur();
    var what = e.tagName + (e.id ? "#" + e.id : "") + (typeof e.className === "string" && e.className ? "." + e.className.split(" ")[0] : "");
    if (window.__perchLog) window.__perchLog("tap " + what + (handled ? " (page handled touchend)" : ""));
    if (handled) return what;
    M("mousedown", 1); M("mouseup", 0);
    // A bare media file (a video opened from Files) has no page script to toggle it.
    if (e.tagName === "VIDEO" && document.body && document.body.children.length === 1) { if (e.paused) e.play(); else e.pause(); }
    else M("click", 0);
    return what;
    """

    static let scroll = """
    var at = window.__perchAt ? window.__perchAt(x, y) : null;
    var e = at ? at.el : null, top = at ? at.win : window;
    while (e && e !== top.document.documentElement && e !== top.document.body) {
        var s = getComputedStyle(e);
        var canY = /(auto|scroll)/.test(s.overflowY) && e.scrollHeight > e.clientHeight;
        var canX = /(auto|scroll)/.test(s.overflowX) && e.scrollWidth > e.clientWidth;
        if ((dy && canY) || (dx && canX)) { e.scrollBy(dx, dy); return; }
        e = e.parentElement || (e.getRootNode && e.getRootNode().host) || null;
    }
    top.scrollBy(dx, dy);
    """

    static let insert = """
    var e = document.activeElement; if (!e) return;
    if (e.isContentEditable) { document.execCommand("insertText", false, t); return; }
    if (!("value" in e)) return;
    var s = e.selectionStart, n = e.selectionEnd;
    if (s == null) e.value += t; else e.setRangeText(t, s, n, "end");
    e.dispatchEvent(new InputEvent("input", {bubbles: true, data: t, inputType: "insertText"}));
    """

    static let backspace = """
    var e = document.activeElement; if (!e) return;
    if (e.isContentEditable) { document.execCommand("delete"); return; }
    if (!("value" in e)) return;
    var s = e.selectionStart, n = e.selectionEnd;
    if (s == null) e.value = e.value.slice(0, -1);
    else if (s !== n) e.setRangeText("", s, n, "end");
    else if (s > 0) e.setRangeText("", s - 1, s, "end");
    e.dispatchEvent(new InputEvent("input", {bubbles: true, inputType: "deleteContentBackward"}));
    """

    static let enter = """
    var e = document.activeElement || document.body;
    var o = {bubbles: true, cancelable: true, key: "Enter", code: "Enter", keyCode: 13, which: 13};
    var go = e.dispatchEvent(new KeyboardEvent("keydown", o));
    e.dispatchEvent(new KeyboardEvent("keypress", o)); e.dispatchEvent(new KeyboardEvent("keyup", o));
    if (!go) return;
    if (e.isContentEditable) document.execCommand("insertLineBreak");
    else if (e.tagName === "TEXTAREA") { e.setRangeText("\\n", e.selectionStart, e.selectionEnd, "end"); e.dispatchEvent(new InputEvent("input", {bubbles: true})); }
    else if (e.form) { if (e.form.requestSubmit) e.form.requestSubmit(); else e.form.submit(); }
    else if (e.click && e !== document.body) e.click();
    """

    static let key = """
    var e = document.activeElement || document.body;
    var o = {bubbles: true, cancelable: true, key: k, code: k, keyCode: c, which: c};
    var go = e.dispatchEvent(new KeyboardEvent("keydown", o)); e.dispatchEvent(new KeyboardEvent("keyup", o));
    if (k === "Escape" && document.fullscreenElement) { document.exitFullscreen(); return; }
    if (!go || (e !== document.body && e.tagName !== "VIDEO")) return;
    // Nothing took the key: arrows scroll the page, as in a browser.
    var step = {ArrowUp: [0, -80], ArrowDown: [0, 80], ArrowLeft: [-80, 0], ArrowRight: [80, 0]}[k];
    if (step) window.scrollBy(step[0], step[1]);
    """

    static let leaveFullscreen = "if (document.fullscreenElement) { document.exitFullscreen(); return true; } return false;"

    /// Controller state from the pop-up or a paired controller.
    static let gamepad = "if (window.__perchPad) window.__perchPad(a, b);"

    // MARK: installed in every page before its own scripts run

    /// "Fullscreen" means filling the window: the glasses display belongs to
    /// the room, not to one page, and WebKit's own fullscreen cannot run on a
    /// display that takes no touches. The element is stretched over the
    /// page and the Fullscreen API reports success.
    static let fullscreenShim = """
    (function () {
      if (window.__perchFS) return; window.__perchFS = true;
      var cur = null;
      var FILL = ";position:fixed!important;left:0!important;top:0!important;right:auto!important;bottom:auto!important;width:100vw!important;height:100vh!important;" +
        "max-width:none!important;max-height:none!important;margin:0!important;z-index:2147483647!important;background:#000!important;";
      var styled = [];
      function style(el, css) { styled.push([el, el.getAttribute("style")]); el.style.cssText += css; }
      function restore() { styled.forEach(function (p) { if (p[1] == null) p[0].removeAttribute("style"); else p[0].setAttribute("style", p[1]); }); styled = []; }
      function log(m) { if (window.__perchLog) window.__perchLog(m); }
      function size(v) { var r = v.getBoundingClientRect(); return Math.round(r.width) + "x" + Math.round(r.height); }
      function mainVideo() { var best = null, area = 0; document.querySelectorAll("video").forEach(function (v) {
        var r = v.getBoundingClientRect(), a = r.width * r.height; if (a > area) { area = a; best = v; } }); return best; }
      function fills(v) { var r = v.getBoundingClientRect(); return r.width >= innerWidth * 0.95 || r.height >= innerHeight * 0.95; }
      // A page that asks for the whole document to go fullscreen (YouTube) lays its
      // player out itself. If it then does not, stretch the player: the largest
      // video, and the box around it that carries its controls.
      window.__perchFill = function () {
        var v = mainVideo(); if (!v || fills(v)) return false;
        // The player box: the nearest ancestor the size of the video (wrappers in
        // between may be empty boxes the video is merely positioned in). It is
        // the element that carries the controls.
        var box = v, r = v.getBoundingClientRect();
        for (var p = v.parentElement; p && p !== document.body; p = p.parentElement) {
          var q = p.getBoundingClientRect();
          if (q.width > r.width + 8 || q.height > r.height + 8) break;
          if (Math.abs(q.width - r.width) <= 8 && Math.abs(q.height - r.height) <= 8) { box = p; break; }
        }
        // Ancestors that transform or contain would trap a fixed box inside themselves.
        for (var a = box.parentElement; a && a !== document.documentElement; a = a.parentElement) {
          var cs = getComputedStyle(a);
          if (cs.transform !== "none" || cs.contain !== "none" || cs.filter !== "none" || cs.willChange === "transform")
            style(a, ";transform:none!important;contain:none!important;filter:none!important;will-change:auto!important;");
        }
        // Everything beside the player, at every level up to the page, steps aside:
        // a z-index cannot lift the player out of a stacking context an ancestor
        // made, so what would sit on top of it (a column of suggestions) is hidden.
        for (var n = box; n.parentElement && n !== document.documentElement; n = n.parentElement) {
          Array.prototype.forEach.call(n.parentElement.children, function (sib) {
            if (sib !== n && !/^(SCRIPT|STYLE|LINK|HEAD|TEMPLATE)$/.test(sib.tagName)) style(sib, ";visibility:hidden!important;");
          });
        }
        style(box, FILL + "visibility:visible!important;");
        // Inside the box the video keeps its place under the controls, at the box's new size.
        if (box !== v) style(v, ";position:fixed!important;width:100vw!important;height:100vh!important;left:0!important;top:0!important;object-fit:contain!important;");
        window.dispatchEvent(new Event("resize"));
        return true;
      };
      // Like the real thing, the change event comes after the call returns.
      function fire(el) { setTimeout(function () { ["fullscreenchange", "webkitfullscreenchange"].forEach(function (n) {
        (el && el.isConnected ? el : document).dispatchEvent(new Event(n, {bubbles: true, composed: true})); }); }, 0); }
      function exit(silent) { if (!cur) return Promise.resolve(); var el = cur; cur = null;
        restore(); log("fullscreen exit");
        if (!silent) { fire(el); setTimeout(function () { window.dispatchEvent(new Event("resize")); }, 50); }
        return Promise.resolve(); }
      function enter(el) { exit(true); cur = el;
        var page = el === document.documentElement || el === document.body;
        if (!page) style(el, FILL);
        fire(el);
        setTimeout(function () { if (cur !== el) return;
          var v = mainVideo(), before = v ? size(v) : "none", forced = page && window.__perchFill();
          log("fullscreen enter " + el.tagName + (el.id ? "#" + el.id : "") + "; video " + before + " in " + innerWidth + "x" + innerHeight +
            (forced ? "; page did not enlarge it, stretched to " + size(mainVideo()) : "")); }, 700);
        return Promise.resolve(); }
      // Pages on touch devices lock the orientation on the way in; a refusal must not abort them.
      try { if (screen.orientation) { screen.orientation.lock = function () { return Promise.resolve(); }; screen.orientation.unlock = function () {}; } } catch (e) {}
      window.addEventListener("unhandledrejection", function (e) { log("rejection: " + String(e.reason && (e.reason.message || e.reason)).slice(0, 160)); });
      var E = Element.prototype, D = Document.prototype, V = HTMLVideoElement.prototype;
      E.requestFullscreen = E.webkitRequestFullscreen = E.webkitRequestFullScreen = function () { return enter(this); };
      V.webkitEnterFullscreen = V.webkitEnterFullScreen = function () { enter(this); };
      V.webkitExitFullscreen = V.webkitExitFullScreen = function () { exit(); };
      D.exitFullscreen = D.webkitExitFullscreen = D.webkitCancelFullScreen = function () { return exit(); };
      function prop(o, names, get) { names.forEach(function (n) { try { Object.defineProperty(o, n, {configurable: true, get: get}); }
        catch (e) { if (window.__perchLog) window.__perchLog("fullscreen: cannot define " + n + ": " + e.message); } }); }
      prop(D, ["fullscreenElement", "webkitFullscreenElement", "webkitCurrentFullScreenElement"], function () { return cur; });
      prop(D, ["fullscreenEnabled", "webkitFullscreenEnabled"], function () { return true; });
      prop(D, ["fullscreen", "webkitIsFullScreen"], function () { return !!cur; });
      prop(V, ["webkitSupportsFullscreen"], function () { return true; });
      prop(V, ["webkitDisplayingFullscreen"], function () { return cur === this; });
      if (window === window.top && window.__perchLog) window.__perchLog("page start: fullscreenEnabled=" + document.fullscreenEnabled +
        " requestFullscreen=" + (Element.prototype.requestFullscreen && /enter\\(this\\)/.test(String(Element.prototype.requestFullscreen))) + " touchPoints=" + navigator.maxTouchPoints);
    })();
    """

    /// What the input scripts share: the element under a point, looking into
    /// shadow roots and same-origin frames, and a log line back to the app
    /// (shown in Check → report) so a page that resists can be diagnosed.
    static let inputShim = """
    (function () {
      if (window.__perchAt) return;
      window.__perchLog = function (m) { try { window.webkit.messageHandlers.perch.postMessage(String(m).slice(0, 240)); } catch (e) {} };
      window.addEventListener("error", function (e) { window.__perchLog("error: " + e.message + " @" + String(e.filename || "").split("/").pop() + ":" + e.lineno); });
      window.__perchAt = function (x, y) {
        var win = window, doc = document, e = doc.elementFromPoint(x, y);
        for (var depth = 0; e && depth < 8; depth++) {
          if (e.shadowRoot && e.shadowRoot.elementFromPoint) { var inner = e.shadowRoot.elementFromPoint(x, y); if (inner && inner !== e) { e = inner; continue; } }
          if (e.tagName === "IFRAME") {
            try { var d = e.contentDocument; if (d) { var r = e.getBoundingClientRect(); x -= r.left; y -= r.top; win = e.contentWindow; doc = d;
              var inside = d.elementFromPoint(x, y); if (inside) { e = inside; continue; } } } catch (err) {}
          }
          break;
        }
        return e ? {el: e, win: win, x: x, y: y} : null;
      };
    })();
    """

    /// Xbox Cloud Gaming and GeForce NOW refuse to start on an iPad unless
    /// the page was launched from the Home Screen ("Add to Home Screen"):
    /// they test `navigator.standalone` and the `display-mode` media query.
    /// A window on the glasses *is* that kind of chrome-less, full-page
    /// launch, so it says so.
    static let standaloneShim = """
    (function () {
      if (window.__perchStandalone) return; window.__perchStandalone = true;
      try { Object.defineProperty(Navigator.prototype, "standalone", {configurable: true, get: function () { return true; }}); } catch (e) {}
      var mm = window.matchMedia ? window.matchMedia.bind(window) : null;
      if (mm) window.matchMedia = function (q) {
        var r = mm(q);
        if (!/display-mode\\s*:\\s*(standalone|fullscreen)/i.test(String(q))) return r;
        return new Proxy(r, {get: function (t, k) { if (k === "matches") return true; var v = t[k]; return typeof v === "function" ? v.bind(t) : v; }});
      };
    })();
    """

    /// A virtual controller for the Gamepad API (cloud gaming polls
    /// `navigator.getGamepads()`). Until the app reports a state, the page
    /// sees whatever WebKit itself offers.
    static let gamepadShim = """
    (function () {
      if (window.__perchPad) return;
      var st = {on: false, ts: 0, axes: [0, 0, 0, 0], buttons: []};
      for (var i = 0; i < 17; i++) st.buttons.push(0);
      var rumble = {type: "dual-rumble", playEffect: function () { return Promise.resolve("complete"); }, reset: function () { return Promise.resolve("complete"); }};
      function pad() { return {id: "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 02fd)", index: 0, connected: true,
        mapping: "standard", timestamp: st.ts, axes: st.axes.slice(), vibrationActuator: rumble, hapticActuators: [],
        buttons: st.buttons.map(function (v) { return {pressed: v > 0.5, touched: v > 0, value: v}; })}; }
      var native = navigator.getGamepads ? navigator.getGamepads.bind(navigator) : null;
      navigator.getGamepads = function () { return st.on ? [pad(), null, null, null] : (native ? native() : [null, null, null, null]); };
      window.__perchPad = function (axes, buttons) {
        var was = st.on; st.on = true; st.axes = axes; st.buttons = buttons; st.ts = performance.now();
        if (!was) { var e = new Event("gamepadconnected"); e.gamepad = pad(); window.dispatchEvent(e); }
      };
    })();
    """
}
