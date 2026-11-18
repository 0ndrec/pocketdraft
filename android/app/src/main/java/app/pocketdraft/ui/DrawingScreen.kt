package app.pocketdraft.ui

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.CheckBoxOutlineBlank
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.FullscreenExit
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PictureAsPdf
import androidx.compose.material.icons.outlined.Anchor
import androidx.compose.material.icons.outlined.AspectRatio
import androidx.compose.material.icons.outlined.EditNote
import androidx.compose.material.icons.outlined.ReportProblem
import androidx.compose.material.icons.outlined.RestoreFromTrash
import androidx.compose.material.icons.outlined.SaveAlt
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.BottomAppBar
import androidx.compose.material3.BottomAppBarDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FloatingActionButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import app.pocketdraft.AppViewModel
import org.json.JSONObject
import uniffi.draftwright_ffi.Doc
import uniffi.draftwright_ffi.viewerHtml
import uniffi.draftwright_ffi.viewerShow

private const val TITLE = "title:"

/** A tapped side of the part (`side:bottom`): an edge in a view, or its datum symbol. */
private const val SIDE = "side:"

/**
 * The open drawing. Material 3 layout for a canvas screen:
 * - top app bar: back, the name, undo, and a menu (datums and origins, how the drawing is
 *   drawn, removed dimensions, Save as DXF, Settings);
 * - the sheet in a WebView, with a sheet switcher floating over it when there are several;
 * - bottom app bar: the drawing's tools as icons, and Save PDF as the screen's main action.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DrawingScreen(vm: AppViewModel) {
    val doc = vm.doc
    val context = LocalContext.current
    var painting by remember { mutableStateOf(false) }
    val savePdf = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/pdf")) { uri ->
        uri?.let { vm.savePdf(context.contentResolver, it) }
    }
    // Android knows no MIME type for .dxf: under a made-up one the provider drops the
    // extension from a renamed copy ("name.dxf (1)"); as plain bytes it keeps the name's own.
    val saveDxf = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        uri?.let { vm.saveDxf(context.contentResolver, it) }
    }
    // The file name: the model's, without the part after " · " or the extension.
    val base = (doc?.name ?: "drawing").substringBefore(" · ").substringBeforeLast('.')
    BackHandler(enabled = doc != null) {
        when {
            vm.selected != null -> vm.selected = null
            vm.showProblems -> vm.showProblems = false
            vm.fullScreen -> vm.fullScreen = false
            else -> vm.close()
        }
    }
    // Full screen: the sheet alone, with one button back to the bars.
    val full = vm.fullScreen && doc != null
    Scaffold(
        topBar = {
            if (!full) TopAppBar(
                navigationIcon = {
                    IconButton(onClick = vm::close) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
                },
                title = {
                    Column {
                        Text(doc?.name ?: "", maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            doc?.let { "${it.faces} faces · ${it.features} features" } ?: "",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                actions = {
                    Tool("Full screen") {
                        IconButton(enabled = doc != null, onClick = { vm.selected = null; vm.fullScreen = true }) {
                            Icon(Icons.Filled.Fullscreen, "Full screen")
                        }
                    }
                    Tool("Undo") {
                        // Off with nothing to take back (as after reopening: the history is the session's).
                        IconButton(enabled = doc?.canUndo == true, onClick = vm::undo) { Icon(Icons.AutoMirrored.Filled.Undo, "Undo") }
                    }
                    More(vm, onSaveDxf = {
                        // The core writes one sheet per DXF: name the sheet when there are several.
                        val sheet = doc?.takeIf { it.sheets.size > 1 }?.let { "-sheet${it.sheet}" } ?: ""
                        saveDxf.launch("$base$sheet.dxf")
                    })
                },
            )
        },
        bottomBar = {
            if (!full) Tools(vm, onSavePdf = { savePdf.launch("$base.pdf") })
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Box(Modifier.fillMaxWidth().height(4.dp)) {
                if ((vm.busy != null && vm.opening == null) || painting) LinearProgressIndicator(Modifier.fillMaxWidth())
            }
            Box(Modifier.weight(1f).fillMaxWidth()) {
                Viewer(vm, onPainting = { painting = it })
                vm.opening?.let { OpeningCard(it, Modifier.align(Alignment.TopCenter).padding(16.dp)) }
                if (doc != null && doc.sheets.size > 1) {
                    SheetSwitcher(vm, doc, Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp))
                }
                if (full) {
                    Box(Modifier.align(Alignment.TopEnd).padding(12.dp)) {
                        Tool("Leave full screen") {
                            FilledTonalIconButton(onClick = { vm.fullScreen = false }) {
                                Icon(Icons.Filled.FullscreenExit, "Leave full screen")
                            }
                        }
                    }
                }
            }
        }
    }
    if (doc != null && vm.showProblems) ProblemsSheet(vm, doc)
    if (doc != null && vm.showDatums) DatumSheet(vm, doc)
    if (doc != null && vm.showRemoved && doc.removed.isNotEmpty()) RemovedSheet(vm, doc)
    val selected = vm.selected
    if (doc != null && selected != null) {
        if (selected.startsWith(TITLE)) {
            TitleSheet(vm, doc, selected.removePrefix(TITLE))
        } else if (selected.startsWith(SIDE)) {
            SideSheet(vm, doc, selected.removePrefix(SIDE).substringBefore('@'))
        } else {
            doc.items.firstOrNull { it.id == selected }?.let { ItemSheet(vm, it) }
        }
    }
}

/**
 * The top bar's overflow menu: the drawing's settings and what is used less often. The tools
 * of every session (problems, paper, title block, PDF, undo) stay on the bars.
 */
@Composable
private fun More(vm: AppViewModel, onSaveDxf: () -> Unit) {
    val doc = vm.doc
    val features = vm.features
    val removed = doc?.removed?.size ?: 0
    val hidden = doc?.hidden ?: true
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(enabled = doc != null, onClick = { open = true }) { Icon(Icons.Filled.MoreVert, "More") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("Datums and origins") },
                leadingIcon = { Icon(Icons.Outlined.Anchor, null) },
                onClick = { open = false; vm.selected = null; vm.showDatums = true },
            )
            DropdownMenuItem(
                text = { Text("Hidden lines") },
                leadingIcon = {
                    Icon(if (hidden) Icons.Filled.CheckBox else Icons.Filled.CheckBoxOutlineBlank, null)
                },
                onClick = { open = false; vm.hidden(!hidden) },
            )
            // Controls for what newer drawing cores can do; hidden while the core cannot.
            if (features.shaded) {
                val shaded = doc?.shaded ?: false
                DropdownMenuItem(
                    text = { Text("Shaded isometric") },
                    leadingIcon = {
                        Icon(if (shaded) Icons.Filled.CheckBox else Icons.Filled.CheckBoxOutlineBlank, null)
                    },
                    onClick = { open = false; vm.shaded(!shaded) },
                )
            }
            if (features.units.size > 1) {
                HorizontalDivider()
                MenuLabel("Units")
                features.units.forEach { u ->
                    DropdownMenuItem(
                        text = { Text(unitName(u)) },
                        leadingIcon = { if (doc?.units == u) Icon(Icons.Filled.Check, null) },
                        onClick = { open = false; if (doc?.units != u) vm.units(u) },
                    )
                }
            }
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text(if (removed > 0) "Removed dimensions · $removed" else "No removed dimensions") },
                leadingIcon = { Icon(Icons.Outlined.RestoreFromTrash, null) },
                enabled = removed > 0,
                onClick = { open = false; vm.selected = null; vm.showRemoved = true },
            )
            if (features.dxf) {
                DropdownMenuItem(
                    text = { Text(if ((doc?.sheets?.size ?: 1) > 1) "Save sheet as DXF" else "Save as DXF") },
                    leadingIcon = { Icon(Icons.Outlined.SaveAlt, null) },
                    onClick = { open = false; onSaveDxf() },
                )
            }
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("Settings") },
                leadingIcon = { Icon(Icons.Outlined.Settings, null) },
                onClick = { open = false; vm.selected = null; vm.showSettings = true },
            )
        }
    }
}

/** A group heading inside a menu. */
@Composable
private fun MenuLabel(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
    )
}

private fun unitName(units: String) = when (units) {
    "mm" -> "Millimetres"
    "in" -> "Inches"
    else -> units
}

/**
 * A tool of the bottom bar: its icon over a short label (Material 3 navigation style), with an
 * optional badge on the icon.
 */
@Composable
private fun BarAction(
    label: String,
    icon: ImageVector,
    enabled: Boolean,
    onClick: () -> Unit,
    badge: (@Composable () -> Unit)? = null,
) {
    val tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
    Column(
        Modifier
            .width(76.dp)
            .clip(MaterialTheme.shapes.medium)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        BadgedBox(badge = { badge?.invoke() }) { Icon(icon, label, tint = tint) }
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = tint,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(top = 4.dp),
        )
    }
}

/** A long press on an icon-only control names it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Tool(label: String, content: @Composable () -> Unit) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
        content = content,
    )
}

/**
 * The bottom app bar: problems (with their count), paper size, the title block,
 * and Save PDF as the floating action button. It stays while no drawing is open (the library
 * covers it), so the sheet above never changes size.
 */
@Composable
private fun Tools(vm: AppViewModel, onSavePdf: () -> Unit) {
    val doc = vm.doc
    val open = doc != null
    BottomAppBar(
        actions = {
            val total = doc?.problemTotal?.toInt() ?: 0
            BarAction("Problems", Icons.Outlined.ReportProblem, open, onClick = { vm.selected = null; vm.showProblems = true }) {
                if (total > 0) Badge { Text(if (total > 99) "99+" else "$total") }
            }
            SizeTool(vm)
            BarAction("Title block", Icons.Outlined.EditNote, open, onClick = { vm.showProblems = false; vm.selected = TITLE + "title" })
        },
        floatingActionButton = {
            Tool("Save PDF") {
                FloatingActionButton(
                    onClick = { if (open) onSavePdf() },
                    containerColor = BottomAppBarDefaults.bottomAppBarFabColor,
                    elevation = FloatingActionButtonDefaults.bottomAppBarFabElevation(),
                ) { Icon(Icons.Filled.PictureAsPdf, "Save PDF") }
            }
        },
    )
}

/** Paper sizes, in a menu over the bar; how a paper is kept is in Settings. */
@Composable
private fun SizeTool(vm: AppViewModel) {
    val doc = vm.doc
    var open by remember { mutableStateOf(false) }
    Box {
        // The label names the paper; a padlock while the drawing keeps its paper and scale.
        BarAction(doc?.paper?.takeIf { it.isNotEmpty() } ?: "Paper", Icons.Outlined.AspectRatio, doc != null, onClick = { open = true }) {
            if (doc?.kept == true) {
                Badge(containerColor = MaterialTheme.colorScheme.secondaryContainer) {
                    Icon(Icons.Filled.Lock, null, Modifier.size(10.dp))
                }
            }
        }
        if (doc == null) return@Box
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            MenuLabel(listOf(doc.paper, doc.scale).filter { it.isNotEmpty() }.joinToString(" at ") + if (doc.kept) " · kept" else "")
            DropdownMenuItem(
                text = { Text(if (doc.size == null) "Automatic · ${doc.paper}" else "Automatic") },
                leadingIcon = { if (doc.size == null) Icon(Icons.Filled.Check, null) },
                onClick = { open = false; vm.size(null) },
            )
            // The sizes the core has; with several paper families, chips choose the family shown
            // (at first the family of the drawing's paper).
            val families = vm.features.papers
            var shown by remember(doc.paper) {
                mutableStateOf(families.firstOrNull { doc.paper.startsWith("${it.name} ") }?.name ?: families.firstOrNull()?.name)
            }
            if (families.size > 1) {
                HorizontalDivider()
                Row(
                    Modifier.padding(horizontal = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    families.forEach { f ->
                        FilterChip(
                            selected = shown == f.name,
                            onClick = { shown = f.name },
                            label = { Text(f.name) },
                        )
                    }
                }
            }
            families.firstOrNull { it.name == shown }?.sizes?.forEach { s ->
                DropdownMenuItem(
                    text = { Text(s) },
                    leadingIcon = { if (doc.size == s) Icon(Icons.Filled.Check, null) },
                    onClick = { open = false; vm.size(s) },
                )
            }
        }
    }
}

/** Previous and next sheet, floating over the drawing; only for drawings of several sheets. */
@Composable
private fun SheetSwitcher(vm: AppViewModel, doc: Doc, modifier: Modifier) {
    val at = doc.sheets.indexOf(doc.sheet)
    Surface(
        modifier = modifier,
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shadowElevation = 3.dp,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(enabled = at > 0, onClick = { vm.sheet(doc.sheets[at - 1]) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, "Previous sheet")
            }
            Text("Sheet ${at + 1} of ${doc.sheets.size}", style = MaterialTheme.typography.labelLarge)
            IconButton(enabled = at < doc.sheets.size - 1, onClick = { vm.sheet(doc.sheets[at + 1]) }) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, "Next sheet")
            }
        }
    }
}

/**
 * The sheet: the viewer page from the Rust core (zoom, pan, tap picking) in a WebView that
 * is made once. The page talks over `window.bridge.postMessage` / `onMessage`; a JavaScript
 * interface carries it.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun Viewer(vm: AppViewModel, onPainting: (Boolean) -> Unit) {
    val background = MaterialTheme.colorScheme.surfaceContainer.hex()
    var ready by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf("") }
    val main = remember { Handler(Looper.getMainLooper()) }
    val view = remember { mutableStateOf<WebView?>(null) }
    // When the last drawing was handed to the page, for the log's timing of its paint.
    val sentAt = remember { longArrayOf(0L) }

    fun send(message: String) {
        view.value?.evaluateJavascript("window.bridge.onMessage(${JSONObject.quote(message)})", null)
    }

    AndroidView(
        modifier = Modifier.fillMaxSize(),
        factory = { context ->
            WebView(context).apply {
                settings.javaScriptEnabled = true
                setBackgroundColor(android.graphics.Color.TRANSPARENT)
                addJavascriptInterface(object {
                    @JavascriptInterface
                    fun post(message: String) {
                        main.post {
                            when {
                                message == "ready" -> { shown = ""; ready = true }
                                message == "drawn" -> {
                                    onPainting(false)
                                    Log.i("Draftwright", "page: painted ${SystemClock.elapsedRealtime() - sentAt[0]} ms after the drawing arrived")
                                }
                                message.startsWith("tap:") -> {
                                    val id = message.removePrefix("tap:")
                                    if (vm.doc != null) vm.selected = id.ifEmpty { null }
                                }
                            }
                        }
                    }
                }, "DwBridge")
                val bridge = "<script>window.bridge={postMessage:function(m){DwBridge.post(m)}};</script>"
                val page = viewerHtml(background).replaceFirst("<head>", "<head>$bridge")
                loadDataWithBaseURL("about:blank", page, "text/html", "utf-8", null)
                view.value = this
            }
        },
    )
    val doc = vm.doc
    LaunchedEffect(ready, doc) {
        if (!ready) return@LaunchedEffect
        if (doc == null) {
            if (shown.isNotEmpty()) {
                shown = ""
                send("clear:")
            }
            return@LaunchedEffect
        }
        val key = "${doc.name}#${doc.sheet}"
        val fit = shown != key
        shown = key
        onPainting(true)
        val started = SystemClock.elapsedRealtime()
        val page = viewerShow(fit, doc.svg)
        val built = SystemClock.elapsedRealtime()
        send(page)
        send(doc.hitsMessage)
        sentAt[0] = started
        Log.i(
            "Draftwright",
            "page: ${page.length / 1024} KB message built in ${built - started} ms, handed over in ${SystemClock.elapsedRealtime() - built} ms",
        )
    }
    LaunchedEffect(ready, vm.selected) {
        if (ready) send("sel:" + (vm.selected ?: ""))
    }
    LaunchedEffect(ready, background) {
        if (ready) send("bg:$background")
    }
}
