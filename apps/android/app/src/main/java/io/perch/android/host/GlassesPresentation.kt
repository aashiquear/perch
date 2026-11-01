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

    /** Add a window; returns a Surface the source should draw into. */
    fun addWindow(kind: Int, w: Int, h: Int, yaw: Float, pitch: Float, distance: Float, widthM: Float, heightM: Float, anchor: Int, cb: (Surface) -> Unit) {
        glView.queueEvent { cb(renderer.addWindow(kind, w, h, yaw, pitch, distance, widthM, heightM, anchor)) }
    }

    override fun onStop() {
        super.onStop()
        glView.queueEvent { renderer.release() }
    }
}

/** One spatial window's GL-side state. */
private class Layer(val index: Int, val texId: Int, val surfaceTexture: SurfaceTexture, val surface: Surface) {
    @Volatile var dirty = false
}

class SpatialRenderer(private val panelW: Int, private val panelH: Int) : GLSurfaceView.Renderer {
    private var space = 0L
    private val layers = ArrayList<Layer>()
    private var program = 0
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
        aPos = GLES20.glGetAttribLocation(program, "aPosW")
        aUv = GLES20.glGetAttribLocation(program, "aUv")
        uTex = GLES20.glGetUniformLocation(program, "uTex")
        uOpacity = GLES20.glGetUniformLocation(program, "uOpacity")
        GLES20.glClearColor(0f, 0f, 0f, 1f)   // black = transparent on the glasses
        GLES20.glEnable(GLES20.GL_BLEND)
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) = GLES20.glViewport(0, 0, width, height)

    fun addWindow(kind: Int, w: Int, h: Int, yaw: Float, pitch: Float, distance: Float, widthM: Float, heightM: Float, anchor: Int): Surface {
        val ids = IntArray(1)
        GLES20.glGenTextures(1, ids, 0)
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, ids[0])
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_LINEAR)
        GLES20.glTexParameteri(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_LINEAR)
        val st = SurfaceTexture(ids[0]).also { it.setDefaultBufferSize(w, h) }
        val index = PerchNative.spaceUpsert(space, -1, kind, w, h, yaw, pitch, distance, widthM, heightM, anchor)
        val layer = Layer(index, ids[0], st, Surface(st))
        st.setOnFrameAvailableListener { layer.dirty = true }
        layers.add(layer)
        return layer.surface
    }

    fun pinHere(index: Int) = PerchNative.spacePinHere(space, index)

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
        for (layer in layers) {
            if (layer.dirty) { layer.surfaceTexture.updateTexImage(); layer.dirty = false }
            val vis = PerchNative.spaceProject(space, layer.index, dt, corners, homog)
            if (vis < 0) continue
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
