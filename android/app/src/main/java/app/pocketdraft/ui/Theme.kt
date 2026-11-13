package app.pocketdraft.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

/** Material 3, with the wallpaper's colours on Android 12+ and a blue scheme before. */
@Composable
fun DraftwrightTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val scheme = when {
        Build.VERSION.SDK_INT >= 31 -> if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> darkColorScheme(primary = Color(0xFFB3C5FF))
        else -> lightColorScheme(primary = Color(0xFF2F5BEA))
    }
    MaterialTheme(colorScheme = scheme, content = content)
}

/** `#RRGGBB` of a colour, for the viewer page. */
fun Color.hex(): String {
    val argb = (red * 255).toInt() shl 16 or ((green * 255).toInt() shl 8) or (blue * 255).toInt()
    return "#%06X".format(argb and 0xFFFFFF)
}
