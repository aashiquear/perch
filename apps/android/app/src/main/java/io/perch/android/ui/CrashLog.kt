package io.perch.android.ui

import android.content.Context
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import java.io.File

/**
 * Keeps the last uncaught exception in `files/crash.txt` and shows it in the
 * panel on the next launch, so a crash during testing is readable on the phone
 * without adb.
 */
object CrashLog {
    val last: MutableStateFlow<String?> = MutableStateFlow(null)
    private fun file(ctx: Context) = File(ctx.filesDir, "crash.txt")

    fun install(ctx: Context) {
        last.value = file(ctx).takeIf { it.exists() }?.readText()
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching {
                val text = "thread ${t.name}\n" + Log.getStackTraceString(e)
                file(ctx).writeText(text.lines().take(40).joinToString("\n"))
            }
            previous?.uncaughtException(t, e)
        }
    }

    fun clear(ctx: Context) { file(ctx).delete(); last.value = null }
}
