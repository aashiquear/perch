package io.perch.android.touchpad

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.json.*
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.util.UUID

/**
 * Controller-side client for `perchd` (crates/perch-net). Speaks the JSON
 * control protocol over WebSocket (docs/PROTOCOL.md) and ships batched input
 * over UDP as postcard (see [Postcard]).
 */
class PerchClient(private val ctx: Context, private val deviceName: String) {
    companion object { private const val TAG = "PerchClient" }

    data class Host(val name: String, val host: String, val port: Int)
    data class WindowInfo(val id: String, val label: String, val focused: Boolean, val visible: Boolean, val yawDeg: Float, val pitchDeg: Float)

    val hosts = MutableStateFlow<List<Host>>(emptyList())
    val connected = MutableStateFlow(false)
    val hostName = MutableStateFlow("")
    val glasses = MutableStateFlow<String?>(null)
    val calibration = MutableStateFlow("uncalibrated")
    val windows = MutableStateFlow<List<WindowInfo>>(emptyList())
    val layouts = MutableStateFlow<List<String>>(emptyList())
    val lastError = MutableStateFlow<String?>(null)

    private val peerId: String = UUID.randomUUID().toString()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val http = OkHttpClient()
    private var ws: WebSocket? = null
    private var udp: DatagramSocket? = null
    private var udpTarget: InetAddress? = null
    private var udpPort = 7421
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    // --- discovery (mDNS via NsdManager; service type from perch-proto) ------
    private var nsd: NsdManager? = null
    private var discovery: NsdManager.DiscoveryListener? = null

    fun startDiscovery() {
        val m = ctx.getSystemService(Context.NSD_SERVICE) as NsdManager
        nsd = m
        val l = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(t: String) {}
            override fun onDiscoveryStopped(t: String) {}
            override fun onStartDiscoveryFailed(t: String, e: Int) { Log.w(TAG, "nsd start failed $e") }
            override fun onStopDiscoveryFailed(t: String, e: Int) {}
            override fun onServiceFound(s: NsdServiceInfo) {
                m.resolveService(s, object : NsdManager.ResolveListener {
                    override fun onResolveFailed(si: NsdServiceInfo, e: Int) {}
                    override fun onServiceResolved(si: NsdServiceInfo) {
                        val h = Host(si.serviceName, si.host.hostAddress ?: return, si.port)
                        hosts.value = (hosts.value.filter { it.name != h.name } + h)
                    }
                })
            }
            override fun onServiceLost(s: NsdServiceInfo) { hosts.value = hosts.value.filter { it.name != s.serviceName } }
        }
        discovery = l
        m.discoverServices("_perch._tcp", NsdManager.PROTOCOL_DNS_SD, l)
    }

    fun stopDiscovery() { discovery?.let { runCatching { nsd?.stopServiceDiscovery(it) } }; discovery = null }

    // --- connection --------------------------------------------------------
    fun connect(host: String, port: Int = 7420, token: String = "") {
        disconnect()
        val req = Request.Builder().url("ws://$host:$port").build()
        ws = http.newWebSocket(req, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: okhttp3.Response) {
                val name = if (token.isEmpty()) deviceName else "$deviceName#$token"
                webSocket.send(hello(name))
                webSocket.send("""{"msg":"command","cmd":"get_state"}""")
                udpTarget = InetAddress.getByName(host)
                udp = DatagramSocket()
                // Subscribe to the pose stream / announce ourselves on UDP.
                sendUdp(Postcard.ping(1))
            }
            override fun onMessage(webSocket: WebSocket, text: String) = handle(text)
            override fun onFailure(webSocket: WebSocket, t: Throwable, response: okhttp3.Response?) {
                connected.value = false; lastError.value = t.message
            }
            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) { connected.value = false }
        })
    }

    fun disconnect() { ws?.close(1000, "bye"); ws = null; udp?.close(); udp = null; connected.value = false }

    private fun hello(name: String): String = buildJsonObject {
        put("msg", "hello")
        put("version", buildJsonObject { put("major", 1); put("minor", 0) })
        put("peer_id", peerId)
        put("name", name)
        put("platform", "android")
        put("caps", buildJsonObject {
            put("glasses_host", false); put("screen_source", false)
            put("input_sink", true); put("input_source", true); put("dex", true)
        })
    }.toString()

    private fun handle(text: String) {
        val o = runCatching { json.parseToJsonElement(text).jsonObject }.getOrNull() ?: return
        when (o["msg"]?.jsonPrimitive?.content) {
            "hello" -> { connected.value = true; hostName.value = o["name"]?.jsonPrimitive?.content ?: "" }
            "event" -> when (o["event"]?.jsonPrimitive?.content) {
                "state" -> applyState(o)
                "glasses_connected" -> glasses.value = o["model"]?.jsonPrimitive?.content
                "glasses_disconnected" -> glasses.value = null
                "calibration" -> calibration.value = o.toString()
                "focus_changed" -> { val id = o["id"]?.jsonPrimitive?.contentOrNull; windows.value = windows.value.map { it.copy(focused = it.id == id) } }
                "window_changed", "window_removed" -> ws?.send("""{"msg":"command","cmd":"get_state"}""")
                "error" -> lastError.value = o["message"]?.jsonPrimitive?.content
            }
            "input" -> {
                // perchd routed input *to us*: this phone is the focused window's peer.
                val evs = o["events"]?.jsonArray ?: return
                io.perch.android.input.InputInjector.inject(evs)
            }
        }
    }

    private fun applyState(o: JsonObject) {
        glasses.value = o["glasses"]?.jsonPrimitive?.contentOrNull
        calibration.value = o["calibration"].toString()
        layouts.value = o["layouts"]?.jsonArray?.map { it.jsonPrimitive.content } ?: emptyList()
        val focused = o["focused"]?.jsonPrimitive?.contentOrNull
        windows.value = o["windows"]?.jsonArray?.map { w ->
            val wo = w.jsonObject
            val p = wo["placement"]!!.jsonObject
            val src = wo["source"]!!.jsonObject
            val label = when (src["kind"]?.jsonPrimitive?.content) {
                "display" -> "Display ${src["display_index"]?.jsonPrimitive?.content}"
                "window" -> src["title"]?.jsonPrimitive?.content ?: "Window"
                "virtual" -> "Virtual ${src["width"]?.jsonPrimitive?.content}×${src["height"]?.jsonPrimitive?.content}"
                "dex" -> "DeX"
                "phone_screen" -> "Phone"
                else -> "?"
            }
            WindowInfo(
                id = wo["id"]!!.jsonPrimitive.content,
                label = label,
                focused = wo["id"]!!.jsonPrimitive.content == focused,
                visible = wo["visible"]?.jsonPrimitive?.boolean ?: true,
                yawDeg = Math.toDegrees(p["yaw"]!!.jsonPrimitive.float.toDouble()).toFloat(),
                pitchDeg = Math.toDegrees(p["pitch"]!!.jsonPrimitive.float.toDouble()).toFloat(),
            )
        } ?: emptyList()
    }

    // --- commands ----------------------------------------------------------
    private fun cmd(body: JsonObjectBuilder.() -> Unit) {
        ws?.send(buildJsonObject { put("msg", "command"); body() }.toString())
    }
    fun recenter() = cmd { put("cmd", "recenter") }
    fun calibrate(seconds: Int = 20) = cmd { put("cmd", "calibrate"); put("seconds", seconds) }
    fun focusNext() = cmd { put("cmd", "focus_next") }
    fun focus(id: String) = cmd { put("cmd", "focus"); put("id", id) }
    fun pinHere(id: String) = cmd { put("cmd", "pin_here"); put("id", id) }
    fun setAnchor(id: String, anchor: String) = cmd { put("cmd", "set_anchor"); put("id", id); put("anchor", anchor) }
    fun preset(name: String) = cmd { put("cmd", "apply_preset"); put("name", name) }
    fun saveLayout(name: String) = cmd { put("cmd", "save_layout"); put("name", name) }
    fun loadLayout(name: String) = cmd { put("cmd", "load_layout"); put("name", name) }
    fun setMode(mode: String) = cmd { put("cmd", "set_display_mode"); put("mode", mode) }
    fun nudge(id: String, yawDeg: Float, pitchDeg: Float, distance: Float, widthM: Float, heightM: Float) = cmd {
        put("cmd", "place_window"); put("id", id)
        put("placement", buildJsonObject {
            put("yaw", Math.toRadians(yawDeg.toDouble())); put("pitch", Math.toRadians(pitchDeg.toDouble()))
            put("distance", distance); put("roll", 0.0); put("width_m", widthM); put("height_m", heightM)
        })
    }

    // --- input (UDP, batched by the touchpad view) ---------------------------
    fun sendInput(batch: ByteArray) = sendUdp(batch)

    private fun sendUdp(bytes: ByteArray) {
        val s = udp ?: return
        val t = udpTarget ?: return
        scope.launch { runCatching { s.send(DatagramPacket(bytes, bytes.size, t, udpPort)) } }
    }
}
