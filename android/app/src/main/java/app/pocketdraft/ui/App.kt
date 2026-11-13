package app.pocketdraft.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.pocketdraft.AppViewModel

/**
 * The drawing screen stays composed for the app's whole run (its WebView loads once); the
 * library is laid over it while no file is open. Compose gives the WebView a normal layout
 * and stops touches at the library's surface, so neither needs a workaround here.
 */
@Composable
fun App(vm: AppViewModel) {
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(vm.message) {
        vm.message?.let {
            val undo = vm.messageUndo
            val result = snackbar.showSnackbar(it, actionLabel = if (undo) "Undo" else null, withDismissAction = true)
            vm.message = null
            if (undo && result == SnackbarResult.ActionPerformed) vm.undo()
        }
    }
    Box(Modifier.fillMaxSize()) {
        DrawingScreen(vm)
        if (vm.doc == null) LibraryScreen(vm)
        if (vm.showSettings) SettingsScreen(vm)
        SnackbarHost(
            snackbar,
            Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 88.dp),
        )
    }
    vm.picking?.let { PartPicker(vm, it) }
}
