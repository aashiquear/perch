package io.perch.android.host

import androidx.compose.runtime.staticCompositionLocalOf
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * Where the pop-up's touchpad, keyboard, d-pad and gamepad skins send their
 * input: the focused glasses window on this phone ([AppWindows]) or a laptop
 * running perchd ([io.perch.android.touchpad.RemoteTarget]). Same UI, two
 * destinations.
 */
interface InputTarget {
    fun cursorMove(dx: Float, dy: Float)
    fun tap()
    fun pressDown()
    fun pressUp()
    /** Wheel lines: dy positive = up, dx positive = right. */
    fun scroll(dx: Float, dy: Float)
    /** Content follows the fingers (trackpad px). */
    fun scrollMove(dx: Float, dy: Float)
    fun scrollEnd()
    fun key(keyCode: Int)
    fun arrow(keyCode: Int, asGamepad: Boolean = false)
    fun typeText(text: String)

    /** X Y Z RZ (−1..1), BRAKE GAS (0..1), HAT_X HAT_Y (−1/0/1). */
    val axes: FloatArray
    fun gamepadAxes(force: Boolean = false)
    fun gamepadButton(keyCode: Int, down: Boolean)
    fun gamepadAnnounce()

    fun focusNext(step: Int)
    /** Operations on the focused window; [supportsWindowOps] says whether they do anything here. */
    val supportsWindowOps: Boolean
    fun pinHere()
    fun flip()
    fun bigger()
    fun smaller()
    fun nearer()
    fun further()
    fun relaunch()

    /** Cursor acts as a mouse (true) or a finger (false); fixed to mouse for a laptop. */
    val mouseMode: MutableStateFlow<Boolean>
    val isLocal: Boolean get() = this === AppWindows
}

val LocalInputTarget = staticCompositionLocalOf<InputTarget> { AppWindows }
