package io.perch.android.shell

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.IBinder
import android.util.Log
import io.perch.android.BuildConfig
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import rikka.shizuku.Shizuku

/**
 * Binds [PerchShellService] through Shizuku and exposes it as a flow. The
 * panel shows [state]; [AppWindows] uses [shell] when it is non-null.
 *
 * Shizuku itself is a separate app the user starts once per boot (via
 * Wireless debugging); see docs/ANDROID.md.
 */
object ShellBridge {
    private const val TAG = "ShellBridge"
    private const val REQ = 7420

    enum class State { NoShizuku, NeedPermission, Binding, Ready, Failed }

    val state: MutableStateFlow<State> = MutableStateFlow(State.NoShizuku)
    private val _shell = MutableStateFlow<IPerchShell?>(null)
    val shell: StateFlow<IPerchShell?> get() = _shell

    private val args by lazy {
        Shizuku.UserServiceArgs(ComponentName(BuildConfig.APPLICATION_ID, PerchShellService::class.java.name))
            .daemon(false)
            .processNameSuffix("shell")
            .debuggable(BuildConfig.DEBUG)
            .version(BuildConfig.VERSION_CODE)
    }

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
            if (!binder.pingBinder()) { state.value = State.Failed; return }
            val s = IPerchShell.Stub.asInterface(binder)
            _shell.value = s
            state.value = State.Ready
            Log.i(TAG, "shell service ready, uid ${runCatching { s.uid() }.getOrDefault(-1)}")
        }
        override fun onServiceDisconnected(name: ComponentName) {
            _shell.value = null
            state.value = State.NoShizuku
        }
    }

    private val permissionListener = Shizuku.OnRequestPermissionResultListener { code, result ->
        if (code == REQ) {
            if (result == PackageManager.PERMISSION_GRANTED) bind() else state.value = State.NeedPermission
        }
    }
    private val binderReceived = Shizuku.OnBinderReceivedListener { refresh() }
    private val binderDead = Shizuku.OnBinderDeadListener {
        _shell.value = null
        state.value = State.NoShizuku
    }

    fun install() {
        Shizuku.addBinderReceivedListenerSticky(binderReceived)
        Shizuku.addBinderDeadListener(binderDead)
        Shizuku.addRequestPermissionResultListener(permissionListener)
    }

    /** Re-evaluate: is Shizuku running, do we have permission, bind if so. */
    fun refresh() {
        if (!Shizuku.pingBinder()) { state.value = State.NoShizuku; return }
        if (Shizuku.isPreV11()) { state.value = State.Failed; return }
        if (Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED) bind()
        else state.value = State.NeedPermission
    }

    fun requestPermission() {
        if (Shizuku.pingBinder() && Shizuku.checkSelfPermission() != PackageManager.PERMISSION_GRANTED) {
            if (Shizuku.shouldShowRequestPermissionRationale()) state.value = State.NeedPermission
            Shizuku.requestPermission(REQ)
        } else refresh()
    }

    private fun bind() {
        if (_shell.value != null) { state.value = State.Ready; return }
        state.value = State.Binding
        try {
            Shizuku.bindUserService(args, connection)
        } catch (e: Throwable) {
            Log.e(TAG, "bindUserService: $e")
            state.value = State.Failed
        }
    }

    fun unbind() {
        runCatching { Shizuku.unbindUserService(args, connection, true) }
        _shell.value = null
    }

    @Suppress("unused")
    fun describe(ctx: Context): String = when (state.value) {
        State.NoShizuku -> "Shizuku not running — start it (Wireless debugging) for multi-app windows"
        State.NeedPermission -> "Tap to allow Perch in Shizuku"
        State.Binding -> "Connecting to Shizuku…"
        State.Ready -> "Shizuku ready (uid ${runCatching { _shell.value?.uid() }.getOrNull() ?: "?"})"
        State.Failed -> "Shizuku helper failed to start"
    }
}
