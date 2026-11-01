package io.perch.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
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

@Composable
fun PerchTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}
