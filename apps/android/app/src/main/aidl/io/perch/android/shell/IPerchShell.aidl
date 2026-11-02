// Perch's privileged helper, run by Shizuku as uid shell. Everything a
// normal app may not do for *other* apps lives behind this interface:
// trusted virtual displays, launching an app onto one, injecting input.
package io.perch.android.shell;

import android.view.Surface;
import android.view.MotionEvent;
import android.view.KeyEvent;

interface IPerchShell {
    /** Create a trusted virtual display rendering into `surface`; returns its display id or -1. */
    int createDisplay(String name, int width, int height, int dpi, in Surface surface) = 1;
    /** Change a display's size (portrait ⇄ landscape); apps get a configuration change. */
    boolean resizeDisplay(int displayId, int width, int height, int dpi) = 2;
    /** Release the display; apps on it are finished by the system. */
    void releaseDisplay(int displayId) = 3;
    /** `am start --display id -n component`; returns the command output (empty on success). */
    String launch(int displayId, String component) = 4;
    /** Inject a touch/mouse event into the display. */
    boolean injectMotion(int displayId, in MotionEvent event) = 5;
    /** Inject a key press/release into the display's focused window. */
    boolean injectKey(int displayId, int keyCode, boolean down, int metaState) = 6;
    /** uid the service runs as (2000 = shell), for the panel's status line. */
    int uid() = 7;
    /** Why the last call failed (exception text), for the panel. */
    String lastError() = 8;
    /** Inject a complete key event (physical keyboard / gamepad), retargeted to the display. */
    boolean injectKeyEvent(int displayId, in KeyEvent event) = 9;
    /** Shizuku convention: tear the service down. */
    void destroy() = 16777114;
}
