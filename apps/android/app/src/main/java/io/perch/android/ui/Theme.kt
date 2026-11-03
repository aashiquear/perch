package io.perch.android.ui

import android.content.Context
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.MutableStateFlow
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * Dark-first: this UI is used next to a pair of glasses in a dim room and,
 * in host mode, often *reflected in* them. High contrast, no pure white.
 */
private val Dark = darkColorScheme(
    primary = Color(0xFF8FD3FF),
    secondary = Color(0xFFB4F0C6),
    background = Color(0xFF0B0F14),
    surface = Color(0xFF121820),
    surfaceVariant = Color(0xFF1B2430),
    onSurfaceVariant = Color(0xFF9AA7B8),
)
private val Light = lightColorScheme(primary = Color(0xFF005E8C), secondary = Color(0xFF2E7D4F))

/** 0 = follow the device, 1 = light, 2 = dark. Persisted in SharedPreferences. */
object ThemeMode {
    val mode: MutableStateFlow<Int> = MutableStateFlow(0)
    fun load(ctx: Context) { mode.value = ctx.getSharedPreferences("perch", Context.MODE_PRIVATE).getInt("theme", 0) }
    fun set(ctx: Context, m: Int) {
        mode.value = m
        ctx.getSharedPreferences("perch", Context.MODE_PRIVATE).edit().putInt("theme", m).apply()
    }
    fun cycle(ctx: Context) = set(ctx, (mode.value + 1) % 3)
    fun label(m: Int) = when (m) { 1 -> "Light"; 2 -> "Dark"; else -> "Device" }
}

@Composable
fun PerchTheme(content: @Composable () -> Unit) {
    val mode by ThemeMode.mode.collectAsStateWithLifecycle()
    val dark = when (mode) { 1 -> false; 2 -> true; else -> isSystemInDarkTheme() }
    MaterialTheme(colorScheme = if (dark) Dark else Light, content = content)
}
