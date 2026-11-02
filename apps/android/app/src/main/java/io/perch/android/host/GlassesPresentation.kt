package io.perch.android.host

import android.app.Presentation
import android.content.Context
import android.graphics.SurfaceTexture
import android.opengl.GLES11Ext
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.view.Display
import android.view.Surface
import android.view.WindowManager
import io.perch.android.native.PerchNative
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10

/**
 * Full-screen GL view on the glasses' display. Draws each spatial window as a
 * projectively-mapped quad (same trick as the desktop compositor: vertex
 * position pre-multiplied by the homography's homogeneous weight) textured
 * from a [SurfaceTexture] that a [WindowSource] (MediaProjection mirror, a
 * virtual display, …) renders into.
 */
class GlassesPresentation(ctx: Context, display: Display) : Presentation(ctx, display) {

    private val glView = GLSurfaceView(ctx)
    val renderer = SpatialRenderer(display.mode.physicalWidth, display.mode.physicalHeight)

    init {
        window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        glView.setEGLContextClientVersion(2)
        glView.preserveEGLContextOnPause = true
        glView.setRenderer(renderer)
        glView.renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        setContentView(glView)
    }

    /** Add a window; the callback gets the layer index and a Surface the source should draw into. */
    fun addWindow(kind: Int, w: Int, h: Int, yaw: Float, pitch: Float, distance: Float, widthM: Float, heightM: Float, anchor: Int, cb: (Int, Surface) -> Unit) {
        glView.queueEvent { val (i, s) = renderer.addWindow(kind, w, h, yaw, pitch, distance, widthM, heightM, anchor); cb(i, s) }
    }

    /** Hide a window (its layer index stays valid). */
    fun hideWindow(index: Int) = glView.queueEvent { renderer.setHidden(index, true) }

    /** Change a window's source pixel size (portrait ⇄ landscape) and its metres accordingly. */
    fun resizeWindow(index: Int, w: Int, h: Int, widthM: Float, heightM: Float) =
        glView.queueEvent { renderer.resizeWindow(index, w, h, widthM, heightM) }

    /** Cursor: which window and where in it (0..1), or null to hide. */
    fun setCursor(index: Int?, u: Float, v: Float) = renderer.setCursor(index, u, v)
    fun pinHere(index: Int) = glView.queueEvent { renderer.pinHere(index) }
    fun nudgeWindow(index: Int, dDistance: Float, scale: Float) = glView.queueEvent { renderer.nudge(index, dDistance, scale) }

    override fun onStop() {
        super.onStop()
        glView.queueEvent { renderer.release() }
    }
}

/** One spatial window's GL-side state. */
private class Layer(val index: Int, val texId: Int, val surfaceTexture: SurfaceTexture, val surface: Surface) {
    @Volatile var dirty = false
    @Volatile var hidden = false
    @Volatile var yaw = 0f
    @Volatile var pitch = 0f
    @Volatile var distance = 2f
    @Volatile var anchor = 0
    @Volatile var kind = 0
    @Volatile var w = 0
    @Volatile var h = 0
    @Volatile var widthM = 1f
    @Volatile var heightM = 1f
}

class SpatialRenderer(private val panelW: Int, private val panelH: Int) : GLSurfaceView.Renderer {
    private var space = 0L
    private val layers = ArrayList<Layer>()
    private var program = 0
    private var solidProgram = 0
    private var sPos = 0
    private var sColor = 0
    @Volatile private var cursorIndex: Int? = null
    @Volatile private var cursorU = 0.5f
    @Volatile private var cursorV = 0.5f
    private var aPos = 0
    private var aUv = 0
    private var uTex = 0
    private var uOpacity = 0
    private var lastFrameNs = 0L
    private val corners = FloatArray(8)
    private val homog = FloatArray(9)
    private val verts: FloatBuffer = ByteBuffer.allocateDirect(6 * 5 * 4).order(ByteOrder.nativeOrder()).asFloatBuffer()

    // Rokid Max optics; override from settings in a later milestone.
    private val fovH = Math.toRadians(46.0).toFloat()
    private val tilt = Math.toRadians(4.0).toFloat()

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        space = PerchNative.spaceNew(panelW, panelH, fovH, tilt)
        program = buildProgram()
        solidProgram = buildSolidProgram()
        sPos = GLES20.glGetAttribLocation(solidProgram, "aPos")
        sColor = GLES20.glGetUniformLocation(solidProgram, "uColor")
        aPos = GLES20.glGetAttribLocation(program, "aPosW")
        aUv = GLES20.glGetAttribLocation(program, "aUv")
        uTex = GLES20.glGetUniformLocation(program, "uTex")
        uOpacity = GLES20.glGetUniformLocation(program, "uOpacity")
        GLES20.glClearColor(0f, 0f, 0f, 1f)   // black = transparent on the glasses
        GLES20.glEnable(GLES20.GL_BLEND)
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) = GLES20.glViewport(0, 0, width, height)

    fun addWindow(kind: Int, w: Int, h: Int, yaw: Float, pitch: Float, distance: Float, widthM: Float, heightM: Float, anchor: Int): Pair<Int, Surface> {
        val ids = IntArray(1)
        GLES20.glGenTextures(1, ids, 0)
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, ids[0])
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_LINEAR)
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_LINEAR)
        val st = SurfaceTexture(ids[0]).also { it.setDefaultBufferSize(w, h) }
        val index = PerchNative.spaceUpsert(space, -1, kind, w, h, yaw, pitch, distance, widthM, heightM, anchor)
        val layer = Layer(index, ids[0], st, Surface(st)).also {
            it.yaw = yaw; it.pitch = pitch; it.distance = distance; it.anchor = anchor; it.kind = kind
            it.w = w; it.h = h; it.widthM = widthM; it.heightM = heightM
        }
        st.setOnFrameAvailableListener { layer.dirty = true }
        layers.add(layer)
        return index to layer.surface
    }

    fun pinHere(index: Int) = PerchNative.spacePinHere(space, index)

    fun setHidden(index: Int, hidden: Boolean) {
        layers.firstOrNull { it.index == index }?.hidden = hidden
    }

    fun resizeWindow(index: Int, w: Int, h: Int, widthM: Float, heightM: Float) {
        val l = layers.firstOrNull { it.index == index } ?: return
        val p = FloatArray(5)
        if (PerchNative.spacePlacement(space, index, p) == 0) { l.yaw = p[0]; l.pitch = p[1]; l.distance = p[2] }
        l.surfaceTexture.setDefaultBufferSize(w, h)
        l.w = w; l.h = h; l.widthM = widthM; l.heightM = heightM
        PerchNative.spaceUpsert(space, index, l.kind, w, h, l.yaw, l.pitch, l.distance, widthM, heightM, l.anchor)
    }

    /** Move a window nearer/further (metres along its ray) and/or scale it (same as the Linux bar). */
    fun nudge(index: Int, dDistance: Float, scale: Float) {
        val l = layers.firstOrNull { it.index == index } ?: return
        // pinHere moved the window inside the space; take that as the starting point.
        val p = FloatArray(5)
        if (PerchNative.spacePlacement(space, index, p) == 0) {
            l.yaw = p[0]; l.pitch = p[1]; l.distance = p[2]; l.widthM = p[3]; l.heightM = p[4]
        }
        l.distance = (l.distance + dDistance).coerceIn(0.6f, 6f)
        l.widthM = (l.widthM * scale).coerceIn(0.2f, 6f)
        l.heightM = (l.heightM * scale).coerceIn(0.2f, 6f)
        PerchNative.spaceUpsert(space, index, l.kind, l.w, l.h, l.yaw, l.pitch, l.distance, l.widthM, l.heightM, l.anchor)
    }

    fun setCursor(index: Int?, u: Float, v: Float) { cursorIndex = index; cursorU = u; cursorV = v }

    override fun onDrawFrame(gl: GL10?) {
        val now = System.nanoTime()
        val dt = if (lastFrameNs == 0L) 0.016f else ((now - lastFrameNs) / 1e9f).coerceAtMost(0.1f)
        lastFrameNs = now

        // Predicted pose for scan-out time.
        val t = GlassesService.tracker
        val pose = FloatArray(8)
        if (t != 0L) PerchNative.trackerPredict(t, 0.004f, pose) else pose[0] = 1f
        PerchNative.spaceSetPose(space, pose)

        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)
        GLES20.glUseProgram(program)
        var cursorPx: FloatArray? = null
        for (layer in layers) {
            if (layer.dirty) { layer.surfaceTexture.updateTexImage(); layer.dirty = false }
            if (layer.hidden) continue
            val vis = PerchNative.spaceProject(space, layer.index, dt, corners, homog)
            if (vis < 0) continue
            if (layer.index == cursorIndex) {
                // Cursor through the same homography: p = H · (u, v, 1).
                val u = cursorU; val v = cursorV
                val w = homog[6] * u + homog[7] * v + homog[8]
                if (w > 1e-4f) cursorPx = floatArrayOf(
                    (homog[0] * u + homog[1] * v + homog[2]) / w,
                    (homog[3] * u + homog[4] * v + homog[5]) / w,
                )
            }
            fillQuad()
            GLES20.glActiveTexture(GLES20.GL_TEXTURE0)
            GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, layer.texId)
            GLES20.glUniform1i(uTex, 0)
            GLES20.glUniform1f(uOpacity, 1f)
            verts.position(0)
            GLES20.glVertexAttribPointer(aPos, 3, GLES20.GL_FLOAT, false, 20, verts)
            GLES20.glEnableVertexAttribArray(aPos)
            verts.position(3)
            GLES20.glVertexAttribPointer(aUv, 2, GLES20.GL_FLOAT, false, 20, verts)
            GLES20.glEnableVertexAttribArray(aUv)
            GLES20.glDrawArrays(GLES20.GL_TRIANGLES, 0, 6)
        }
        cursorPx?.let { drawCursor(it[0], it[1]) }
    }

    /** A small arrow pointer with a dark outline, drawn in panel pixels. */
    private fun drawCursor(x: Float, y: Float) {
        GLES20.glUseProgram(solidProgram)
        fun tri(pts: FloatArray, r: Float, g: Float, b: Float) {
            verts.position(0)
            for (i in 0 until pts.size / 2) {
                verts.put(pts[i * 2] / panelW * 2f - 1f).put(1f - pts[i * 2 + 1] / panelH * 2f).put(1f).put(0f).put(0f)
            }
            verts.position(0)
            GLES20.glVertexAttribPointer(sPos, 3, GLES20.GL_FLOAT, false, 20, verts)
            GLES20.glEnableVertexAttribArray(sPos)
            GLES20.glUniform4f(sColor, r, g, b, 1f)
            GLES20.glDrawArrays(GLES20.GL_TRIANGLES, 0, pts.size / 2)
        }
        val s = 18f
        val arrow = floatArrayOf(x, y, x, y + s * 1.4f, x + s, y + s)
        tri(floatArrayOf(x - 2f, y - 3f, x - 2f, y + s * 1.4f + 4f, x + s + 4f, y + s + 1f), 0.05f, 0.05f, 0.08f)
        tri(arrow, 1f, 1f, 1f)
    }

    private fun buildSolidProgram(): Int {
        val vs = "attribute vec3 aPos; void main() { gl_Position = vec4(aPos.x, aPos.y, 0.0, 1.0); }"
        val fs = "precision mediump float; uniform vec4 uColor; void main() { gl_FragColor = uColor; }"
        fun compile(type: Int, src: String): Int {
            val sh = GLES20.glCreateShader(type)
            GLES20.glShaderSource(sh, src); GLES20.glCompileShader(sh)
            return sh
        }
        val p = GLES20.glCreateProgram()
        GLES20.glAttachShader(p, compile(GLES20.GL_VERTEX_SHADER, vs))
        GLES20.glAttachShader(p, compile(GLES20.GL_FRAGMENT_SHADER, fs))
        GLES20.glLinkProgram(p)
        return p
    }

    /** Same math as perch-compositor::quad_to_vertices. */
    private fun fillQuad() {
        val uv = floatArrayOf(0f, 0f, 1f, 0f, 1f, 1f, 0f, 1f)
        val order = intArrayOf(0, 1, 2, 0, 2, 3)
        verts.position(0)
        for (i in order) {
            val u = uv[i * 2]; val v = uv[i * 2 + 1]
            val w = homog[6] * u + homog[7] * v + homog[8]
            val x = corners[i * 2] / panelW * 2f - 1f
            val y = 1f - corners[i * 2 + 1] / panelH * 2f
            verts.put(x * w).put(y * w).put(w).put(u).put(v)
        }
        verts.position(0)
    }

    fun release() {
        layers.forEach { it.surface.release(); it.surfaceTexture.release() }
        layers.clear()
        if (space != 0L) { PerchNative.spaceFree(space); space = 0L }
    }

    private fun buildProgram(): Int {
        val vs = """
            attribute vec3 aPosW;
            attribute vec2 aUv;
            varying vec2 vUv;
            void main() {
                gl_Position = vec4(aPosW.x, aPosW.y, 0.0, aPosW.z);
                vUv = aUv;
            }
        """.trimIndent()
        val fs = """
            #extension GL_OES_EGL_image_external : require
            precision mediump float;
            uniform samplerExternalOES uTex;
            uniform float uOpacity;
            varying vec2 vUv;
            void main() {
                vec4 c = texture2D(uTex, vUv);
                gl_FragColor = vec4(c.rgb, c.a * uOpacity);
            }
        """.trimIndent()
        fun compile(type: Int, src: String): Int {
            val s = GLES20.glCreateShader(type)
            GLES20.glShaderSource(s, src)
            GLES20.glCompileShader(s)
            val ok = IntArray(1)
            GLES20.glGetShaderiv(s, GLES20.GL_COMPILE_STATUS, ok, 0)
            check(ok[0] != 0) { "shader: " + GLES20.glGetShaderInfoLog(s) }
            return s
        }
        val p = GLES20.glCreateProgram()
        GLES20.glAttachShader(p, compile(GLES20.GL_VERTEX_SHADER, vs))
        GLES20.glAttachShader(p, compile(GLES20.GL_FRAGMENT_SHADER, fs))
        GLES20.glLinkProgram(p)
        return p
    }
}
