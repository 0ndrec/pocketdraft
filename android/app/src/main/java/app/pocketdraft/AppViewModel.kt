package app.pocketdraft

import android.app.Application
import android.content.ContentResolver
import android.net.Uri
import android.os.SystemClock
import android.provider.OpenableColumns
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.draftwright_ffi.Doc
import uniffi.draftwright_ffi.DwException
import uniffi.draftwright_ffi.Entry
import uniffi.draftwright_ffi.Item
import uniffi.draftwright_ffi.PartRow
import uniffi.draftwright_ffi.TitleChange
import uniffi.draftwright_ffi.applyOps
import uniffi.draftwright_ffi.answerProblem
import uniffi.draftwright_ffi.editItem
import uniffi.draftwright_ffi.creditsDraftwright
import uniffi.draftwright_ffi.exactPaper
import uniffi.draftwright_ffi.keepSheet as coreKeepSheet
import uniffi.draftwright_ffi.libraryEntries
import uniffi.draftwright_ffi.looksLikeStep
import uniffi.draftwright_ffi.openFile
import uniffi.draftwright_ffi.openSaved
import uniffi.draftwright_ffi.parts
import uniffi.draftwright_ffi.pdf
import uniffi.draftwright_ffi.dxf
import uniffi.draftwright_ffi.CoreFeatures
import uniffi.draftwright_ffi.coreFeatures
import uniffi.draftwright_ffi.setShaded
import uniffi.draftwright_ffi.setUnits
import uniffi.draftwright_ffi.removeDimension as coreRemoveDimension
import uniffi.draftwright_ffi.removeEntry
import uniffi.draftwright_ffi.restoreDimension as coreRestoreDimension
import uniffi.draftwright_ffi.setCredit
import uniffi.draftwright_ffi.setDatum
import uniffi.draftwright_ffi.setExactPaper
import uniffi.draftwright_ffi.setKeepSheet as coreSetKeepSheet
import uniffi.draftwright_ffi.setHidden
import uniffi.draftwright_ffi.setOrigin
import uniffi.draftwright_ffi.setPaper
import uniffi.draftwright_ffi.setTitleDefaults
import uniffi.draftwright_ffi.setTitles
import uniffi.draftwright_ffi.setUncitedDatums
import uniffi.draftwright_ffi.showSheet
import uniffi.draftwright_ffi.undo as coreUndo
import uniffi.draftwright_ffi.init as coreInit

/** What to tell the user about a failed core call. */
fun DwException.text(): String = when (this) {
    is DwException.Failed -> reason
    is DwException.NothingToUndo -> "Nothing to undo"
}

/** A STEP assembly waiting for the user to pick a part (or the whole file). */
data class Picking(val name: String, val bytes: ByteArray, val parts: List<PartRow>)

/**
 * The app's state and every call into the Rust core. Core calls block until the engine
 * thread answers, so they run on [Dispatchers.Default]; one runs at a time ([busy]).
 */
class AppViewModel(app: Application) : AndroidViewModel(app) {
    var entries by mutableStateOf<List<Entry>>(emptyList())
        private set
    var doc by mutableStateOf<Doc?>(null)
        private set
    /** Text of the running operation; `null` when idle. */
    var busy by mutableStateOf<String?>(null)
        private set
    /** News or an error, shown in a snackbar. */
    var message by mutableStateOf<String?>(null)
    var exact by mutableStateOf(false)
        private set
    /** "Keep paper and scale": edits plan on the sheet the drawing has (on by default). */
    var keepSheet by mutableStateOf(true)
        private set
    /** "Credit draftwright in Drawn by" (on by default). */
    var credit by mutableStateOf(true)
        private set
    var showSettings by mutableStateOf(false)
    /** What the drawing core can do: the controls of features it lacks stay hidden. */
    var features by mutableStateOf(CoreFeatures(papers = emptyList(), units = listOf("mm"), shaded = false, dxf = false))
        private set
    var picking by mutableStateOf<Picking?>(null)
        private set
    /** The item picked on the drawing (`title:<key>` for the title block), or `null`. */
    var selected by mutableStateOf<String?>(null)
    var showProblems by mutableStateOf(false)
    var showDatums by mutableStateOf(false)
    var showRemoved by mutableStateOf(false)
    /** The message is about an edit: its snackbar offers Undo. */
    var messageUndo by mutableStateOf(false)

    private var started = false

    fun start(filesDir: String) {
        if (started) return
        started = true
        coreInit(filesDir)
        exact = exactPaper()
        keepSheet = coreKeepSheet()
        credit = creditsDraftwright()
        features = coreFeatures()
        refresh()
    }

    fun refresh() {
        viewModelScope.launch {
            entries = withContext(Dispatchers.IO) { libraryEntries() }
        }
    }

    /** Run a core operation that answers with a drawing. */
    private fun run(label: String, then: (Doc) -> Unit = {}, op: () -> Doc) {
        if (busy != null) return
        busy = label
        message = null
        messageUndo = false
        viewModelScope.launch {
            try {
                val started = SystemClock.elapsedRealtime()
                val result = withContext(Dispatchers.Default) { op() }
                // With the engine's own note beside it, this shows what the call across costs.
                Log.i("Draftwright", "ui: \"$label\" answered in ${SystemClock.elapsedRealtime() - started} ms")
                val before = doc
                doc = result
                fitNotice(before, result)?.let { message = it }
                then(result)
            } catch (e: DwException.NothingToUndo) {
                message = "Nothing to undo"
            } catch (e: DwException) {
                // A sheet would cover the snackbar: close it so the refusal is read.
                selected = null
                showProblems = false
                showDatums = false
                showRemoved = false
                message = e.text()
            } finally {
                busy = null
                refresh()
            }
        }
    }

    fun openUri(resolver: ContentResolver, uri: Uri) {
        if (busy != null) return
        busy = "Reading the file…"
        viewModelScope.launch {
            try {
                val (name, bytes) = withContext(Dispatchers.IO) {
                    val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                        ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
                        ?: uri.lastPathSegment ?: "model.step"
                    name to (resolver.openInputStream(uri)?.use { it.readBytes() } ?: ByteArray(0))
                }
                if (!looksLikeStep(bytes)) {
                    message = "\"$name\" is not a STEP file"
                    return@launch
                }
                val found = withContext(Dispatchers.Default) { parts(name, bytes) }
                busy = null
                if (found.size > 1) {
                    picking = Picking(name, bytes, found)
                } else {
                    open(name, bytes, null)
                }
            } catch (e: Exception) {
                message = (e as? DwException)?.text() ?: e.message ?: "Could not read the file"
            } finally {
                if (busy == "Reading the file…") busy = null
            }
        }
    }

    /** Open the picked part (`null`: the whole file). */
    fun pick(part: PartRow?) {
        val p = picking ?: return
        picking = null
        open(p.name, p.bytes, part)
    }

    fun cancelPicking() {
        picking = null
    }

    private fun open(name: String, bytes: ByteArray, part: PartRow?) {
        selected = null
        showProblems = false
        showDatums = false
        showRemoved = false
        run("Building the drawing…") { openFile(name, bytes, part?.index, part?.name) }
    }

    fun openEntry(id: String) {
        selected = null
        showProblems = false
        showDatums = false
        showRemoved = false
        run("Opening…") { openSaved(id) }
    }

    fun remove(id: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { removeEntry(id) }
            } catch (e: DwException) {
                message = e.text()
            }
            refresh()
        }
    }

    fun close() {
        if (busy != null) return
        selected = null
        showProblems = false
        showDatums = false
        showRemoved = false
        doc = null
        refresh()
    }

    fun sheet(n: UInt) = run("Drawing the sheet…") { showSheet(n) }

    /**
     * Choose the paper: tried as it is and, when the drawing does not fit it, kept as the
     * largest preferred paper instead (unless Exact size is on).
     */
    fun size(size: String?) {
        val exactNow = exact
        run("Laying out the sheet…", then = { d -> message = paperNotice(d, exactNow) }) {
            setPaper(size, exactNow)
        }
    }

    fun toggleExact() {
        exact = !exact
        viewModelScope.launch(Dispatchers.IO) {
            try {
                setExactPaper(exact)
            } catch (_: DwException) {
            }
        }
        doc?.size?.let { size(it) }
    }

    /**
     * Turn "Keep paper and scale" on or off. Off, edits choose the paper and scale again; the
     * open drawing keeps the sheet it has until Automatic is chosen in the paper menu.
     */
    fun toggleKeepSheet() {
        keepSheet = !keepSheet
        val on = keepSheet
        viewModelScope.launch(Dispatchers.IO) {
            try {
                coreSetKeepSheet(on)
            } catch (_: DwException) {
            }
        }
        val d = doc
        message = when {
            on -> "Edits keep the paper and scale"
            d != null && d.kept -> "This drawing stays on ${d.paper} at ${d.scale}. Choose Automatic to lay it out again"
            else -> "Edits choose the paper and scale again"
        }
    }

    /** Turn the credit in the Drawn by cell on or off, and draw the open sheet again. */
    fun toggleCredit() {
        credit = !credit
        val on = credit
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { setCredit(on) }
            } catch (e: DwException) {
                message = e.text()
            }
            doc?.let { d -> run("Drawing the sheet…") { showSheet(d.sheet) } }
        }
    }

    /** Store the title-block values new drawings start with. */
    fun saveDefaults(values: List<TitleChange>) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { setTitleDefaults(values) }
            } catch (e: DwException) {
                message = e.text()
            }
        }
    }

    fun hidden(on: Boolean) = run("Laying out the sheet…") { setHidden(on) }

    /** A side tapped on the drawing closes its sheet once the datum is set. */
    fun datum(side: String, letter: String?) =
        run("Laying out the sheet…", then = { if (selected?.startsWith("side:$side@") == true) selected = null }) { setDatum(side, letter) }

    fun origin(axis: String, end: String?) = run("Laying out the sheet…") { setOrigin(axis, end) }

    /** The drawing's length units; only offered when the core has more than one. */
    fun units(units: String) = run("Laying out the sheet…") { setUnits(units) }

    /** The isometric view shaded or as lines; only offered when the core can shade. */
    fun shaded(on: Boolean) = run("Drawing the sheet…") { setShaded(on) }

    fun uncitedDatums(shown: Boolean) = run("Laying out the sheet…") { setUncitedDatums(shown) }

    /** Remove a dimension from the drawing; the snackbar offers to undo it. */
    fun removeDimension(item: Item) = run("Laying out the sheet…", then = {
        selected = null
        message = "Removed ${item.label.ifBlank { "the dimension" }}"
        messageUndo = true
    }) { coreRemoveDimension(item.id) }

    fun restoreDimension(id: String) =
        run("Laying out the sheet…", then = { d -> if (d.removed.isEmpty()) showRemoved = false }) { coreRestoreDimension(id) }

    fun fix(opsJson: String) = run("Applying the fix…", then = { showProblems = false }) { applyOps(opsJson) }

    fun answer(problemKey: String, value: String) =
        run("Applying…", then = { showProblems = false }) { answerProblem(problemKey, value) }

    fun edit(item: Item, tolerance: String, fit: String, prefix: String, suffix: String) =
        run("Applying…", then = { selected = null }) { editItem(item, tolerance, fit, prefix, suffix) }

    fun titles(changes: List<TitleChange>) =
        run("Applying…", then = { selected = null }) { setTitles(changes) }

    fun undo() = run("Undoing…") { coreUndo() }

    fun savePdf(resolver: ContentResolver, uri: Uri) = save(resolver, uri, "PDF") { pdf() }

    /** The shown sheet as DXF. */
    fun saveDxf(resolver: ContentResolver, uri: Uri) = save(resolver, uri, "DXF") { dxf() }

    private fun save(resolver: ContentResolver, uri: Uri, format: String, make: () -> ByteArray) {
        if (busy != null) return
        busy = "Making the $format…"
        viewModelScope.launch {
            try {
                val bytes = withContext(Dispatchers.Default) { make() }
                withContext(Dispatchers.IO) { resolver.openOutputStream(uri)?.use { it.write(bytes) } }
                message = "$format saved"
            } catch (e: Exception) {
                message = (e as? DwException)?.text() ?: e.message ?: "Could not save the $format"
            } finally {
                busy = null
            }
        }
    }

    /** A kept sheet the drawing no longer fits after an edit: say how to fit it again. */
    private fun fitNotice(before: Doc?, after: Doc): String? {
        if (!after.kept || !after.overflow || before == null || before.overflow) return null
        return "The drawing no longer fits ${after.paper} at ${after.scale}. Choose Automatic in the paper menu to fit it again"
    }

    private fun paperNotice(d: Doc, exact: Boolean): String? {
        val chosen = d.size ?: return null
        return if (exact) {
            if (d.overflow) "The drawing does not fit $chosen. Turn off Exact size to let it grow" else null
        } else if (d.paper != chosen) {
            "Too dense for $chosen: drawn on ${d.paper}"
        } else {
            null
        }
    }
}
