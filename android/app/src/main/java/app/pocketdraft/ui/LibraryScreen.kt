package app.pocketdraft.ui

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Architecture
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.caverock.androidsvg.SVG
import app.pocketdraft.AppViewModel
import uniffi.draftwright_ffi.Entry

fun Modifier.clickableRow(onClick: () -> Unit) = clickable(onClick = onClick)

/** Home: the drawings opened before, newest first, with their edits. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LibraryScreen(vm: AppViewModel) {
    val context = LocalContext.current
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { vm.openUri(context.contentResolver, it) }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Library") },
                actions = {
                    IconButton(onClick = { vm.showSettings = true }) { Icon(Icons.Outlined.Settings, "Settings") }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { pick.launch(arrayOf("*/*")) },
                icon = { Icon(Icons.Filled.FolderOpen, null) },
                text = { Text("Open STEP file") },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Box(Modifier.fillMaxWidth().padding(vertical = 0.dp)) {
                if (vm.busy != null && vm.opening == null) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
            vm.opening?.let { OpeningCard(it, Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) }
            if (vm.entries.isEmpty()) {
                // The first drawing being made: its progress card says enough.
                if (vm.opening == null) EmptyLibrary()
            } else {
                LazyColumn(contentPadding = PaddingValues(top = 8.dp, bottom = 96.dp)) {
                    items(vm.entries, key = { it.id }) { entry -> EntryRow(vm, entry) }
                }
            }
        }
    }
}

@Composable
private fun EntryRow(vm: AppViewModel, entry: Entry) {
    var menu by remember { mutableStateOf(false) }
    ListItem(
        modifier = Modifier.clickable { vm.openEntry(entry.id) },
        leadingContent = { Thumbnail(entry.thumbnailSvg) },
        headlineContent = { Text(entry.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = { Text(entry.summary, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        trailingContent = {
            Box {
                IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, "Actions") }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text("Remove from library") }, onClick = {
                        menu = false
                        vm.remove(entry.id)
                    })
                }
            }
        },
    )
}

/** The isometric thumbnail, rasterised once per SVG (performance over quality). */
@Composable
private fun Thumbnail(svg: String?) {
    val bitmap = remember(svg) {
        svg?.let {
            runCatching {
                val doc = SVG.getFromString(it)
                val px = 192
                doc.documentWidth = px.toFloat()
                doc.documentHeight = px.toFloat()
                Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888).also { b -> doc.renderToCanvas(Canvas(b)) }
            }.getOrNull()
        }
    }
    Box(
        Modifier.size(72.dp).clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        // The outline is drawn black; the tint gives it the theme colour, light or dark.
        bitmap?.let {
            Image(
                it.asImageBitmap(),
                null,
                Modifier.size(64.dp),
                colorFilter = ColorFilter.tint(MaterialTheme.colorScheme.onSurfaceVariant),
            )
        }
    }
}

/** No drawings yet: what the app does and where to start. */
@Composable
private fun EmptyLibrary() {
    androidx.compose.foundation.layout.Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Outlined.Architecture,
            null,
            Modifier.padding(bottom = 16.dp).size(64.dp),
            tint = MaterialTheme.colorScheme.primary,
        )
        Text("No drawings yet", style = MaterialTheme.typography.titleLarge)
        Text(
            "Open a STEP model (.step, .stp) and Pocketdraft lays out its views, dimensions and title block. Drawings stay here with their edits.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp),
        )
    }
}
