package app.pocketdraft.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Build
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import app.pocketdraft.AppViewModel
import uniffi.draftwright_ffi.Doc
import uniffi.draftwright_ffi.Field
import uniffi.draftwright_ffi.Item
import uniffi.draftwright_ffi.Problem
import uniffi.draftwright_ffi.TitleChange
import uniffi.draftwright_ffi.itemFields
import uniffi.draftwright_ffi.titleFields
import uniffi.draftwright_ffi.unsupportedText

/** Problems of the same kind folded under one heading from this many on. */
private const val FOLD = 3

/** Whether a problem has something the user can do here: a ready fix or a value to enter. */
private fun Problem.fixable() = fixes.isNotEmpty() || ask != null

/**
 * The drawing's problems, in the core's groups; problems of one kind fold under a heading
 * when there are several. Each opens to its ready fixes or the value it asks for, and those
 * that can be fixed here say so.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProblemsSheet(vm: AppViewModel, doc: Doc) {
    var expanded by remember { mutableStateOf<String?>(null) }
    val unfolded = remember { mutableStateListOf<String>() }
    // Opens at full height: an opened problem's field and fixes would lie below a half sheet.
    ModalBottomSheet(
        onDismissRequest = { vm.showProblems = false },
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        val fixable = doc.problems.count { it.fixable() }
        Text(
            "Problems · ${doc.problemTotal} · $fixable fixable",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
        )
        LazyColumn(Modifier.fillMaxWidth().imePadding()) {
            // The core's groups, in its order; a core without them gives one plain list.
            doc.problems.groupBy { it.tier }.forEach { (tier, rows) ->
                tierName(tier)?.let { name ->
                    item(key = "tier:$tier") {
                        Text(
                            "$name · ${rows.size}",
                            style = MaterialTheme.typography.titleSmall,
                            color = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 4.dp),
                        )
                    }
                }
                rows.groupBy { it.code }.forEach { (code, same) ->
                    val fold = "$tier/$code"
                    if (same.size >= FOLD) {
                        item(key = "fold:$fold") {
                            FoldRow(same, open = fold in unfolded) {
                                if (fold in unfolded) unfolded.remove(fold) else unfolded.add(fold)
                            }
                        }
                        if (fold !in unfolded) return@forEach
                    }
                    items(same, key = { it.key }) { p ->
                        ProblemRow(vm, p, expanded == p.key, nested = same.size >= FOLD) {
                            expanded = if (expanded == p.key) null else p.key
                        }
                    }
                }
            }
            item { Spacer(Modifier.height(24.dp)) }
        }
    }
}

/** The heading of one of the core's groups of problems, or `null` for none. */
private fun tierName(tier: String): String? = when (tier) {
    "fix_before_printing" -> "Fix before printing"
    "tidy" -> "Tidy"
    "information" -> "Information"
    else -> null
}

/** Several problems of one kind under one heading, with how many and how many can be fixed. */
@Composable
private fun FoldRow(same: List<Problem>, open: Boolean, toggle: () -> Unit) {
    val first = same.first()
    val fixable = same.count { it.fixable() }
    ListItem(
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickableRow(toggle),
        leadingContent = { SeverityIcon(first.severity) },
        headlineContent = { Text("${first.headline} · ${same.size}") },
        supportingContent = {
            Text(if (fixable > 0) "$fixable can be fixed here" else "Review these on the drawing")
        },
        trailingContent = { Icon(if (open) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore, if (open) "Fold" else "Unfold") },
    )
}

@Composable
private fun SeverityIcon(severity: String) {
    val icon = when (severity) {
        "error" -> Icons.Filled.Error
        "warning" -> Icons.Filled.Warning
        else -> Icons.Filled.Info
    }
    Icon(icon, null, tint = if (severity == "error") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
}

/** A small tonal mark on a row: this problem can be fixed here. */
@Composable
private fun FixMark() {
    Surface(shape = MaterialTheme.shapes.small, color = MaterialTheme.colorScheme.secondaryContainer) {
        Row(Modifier.padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.Build, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
            Text("Fix", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.padding(start = 4.dp))
        }
    }
}

@Composable
private fun ProblemRow(vm: AppViewModel, p: Problem, open: Boolean, nested: Boolean, toggle: () -> Unit) {
    val indent = if (nested) 16.dp else 0.dp
    ListItem(
        // Clear, so the row takes the tone of the sheet.
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.padding(start = indent).clickableRow(toggle),
        leadingContent = { SeverityIcon(p.severity) },
        headlineContent = { Text(p.headline) },
        supportingContent = { Text(p.message, maxLines = if (open) Int.MAX_VALUE else 2) },
        trailingContent = if (p.fixable() && !open) ({ FixMark() }) else null,
    )
    if (!open) return
    Column(Modifier.padding(start = 72.dp + indent, end = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        p.ask?.let { field ->
            var value by remember { mutableStateOf("") }
            OutlinedTextField(
                value = value,
                onValueChange = { value = it },
                label = { Text(field.label) },
                supportingText = { Text(field.hint) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { if (value.isNotBlank()) vm.answer(p.key, value) }),
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { value = it }
            Button(enabled = value.isNotBlank(), onClick = { vm.answer(p.key, value) }) { Text("Apply") }
        }
        p.fixes.forEach { fix ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(fix.description, style = MaterialTheme.typography.bodyMedium)
                    Text(fix.scope, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                FilledTonalButton(onClick = { vm.fix(fix.opsJson) }) { Text("Fix") }
            }
        }
    }
}

@Composable
private fun Suggestions(field: Field, onPick: (String) -> Unit) {
    if (field.suggestions.isEmpty()) return
    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        field.suggestions.forEach { s -> SuggestionChip(onClick = { onPick(s) }, label = { Text(s) }) }
    }
}

/**
 * An editor sheet's heading with Cancel and Apply beside it: at the top, the buttons stay in
 * sight while the keyboard covers the lower part of the sheet.
 */
@Composable
private fun EditorHeader(title: String, onCancel: () -> Unit, onApply: () -> Unit, applyEnabled: Boolean = true) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        TextButton(onClick = onCancel) { Text("Cancel") }
        Button(enabled = applyEnabled, onClick = onApply) { Text("Apply") }
    }
}

/**
 * Why text before or after a value is refused, or `null` when it is fine: the core keeps
 * every number inside the value, so wording may not carry one.
 */
private fun wordingError(key: String, text: String): String? =
    if ((key == "dim:prefix" || key == "dim:suffix") && text.any { it.isDigit() }) {
        "Words only: numbers belong to the value itself (the drawing core refuses them here)."
    } else {
        null
    }

/** A diameter takes a fit; other sizes do not. */
private fun Item.isDiameter() = id.contains("diameter") || label.trimStart().let { it.startsWith("⌀") || it.startsWith("ø") || it.startsWith("Ø") }

/**
 * A dimension or label picked on the drawing: tolerance, fit (diameters only), text before
 * and after. A size takes a tolerance or a fit, not both: entering one clears the other.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ItemSheet(vm: AppViewModel, item: Item) {
    val fields = remember(item.kind, item.id) {
        itemFields(item.kind).filter { it.key != "dim:fit" || item.isDiameter() || !item.fit.isNullOrEmpty() }
    }
    val start = mapOf(
        "dim:tolerance" to (item.tolerance ?: ""),
        "dim:fit" to (item.fit ?: ""),
        "dim:prefix" to (item.prefix ?: "").trim(),
        "dim:suffix" to (item.suffix ?: "").trim(),
    )
    val values = remember(item.id) { mutableStateMapOf<String, String>().apply { putAll(start) } }
    var active by remember(item.id) { mutableStateOf(fields.first().key) }
    // Why a field was emptied, shown under it until the next change.
    var cleared by remember(item.id) { mutableStateOf<String?>(null) }
    val invalid = values.entries.firstOrNull { wordingError(it.key, it.value) != null }?.key
    val apply = {
        if (invalid != null) active = invalid else vm.edit(
            item,
            values["dim:tolerance"] ?: "",
            values["dim:fit"] ?: "",
            values["dim:prefix"] ?: "",
            values["dim:suffix"] ?: "",
        )
    }
    fun set(key: String, value: String) {
        values[key] = value
        cleared = null
        val other = when (key) {
            "dim:fit" -> "dim:tolerance"
            "dim:tolerance" -> "dim:fit"
            else -> null
        }
        if (other != null && value.isNotBlank() && !values[other].isNullOrBlank()) {
            values[other] = ""
            cleared = if (key == "dim:fit") "The fit replaces the tolerance ${start["dim:tolerance"]}." else "The tolerance replaces the fit."
        }
    }
    ModalBottomSheet(onDismissRequest = { vm.selected = null }) {
        Column(Modifier.padding(horizontal = 24.dp).imePadding(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            EditorHeader(item.title, onCancel = { vm.selected = null }, onApply = apply, applyEnabled = invalid == null)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                fields.forEach { f -> FilterChip(selected = active == f.key, onClick = { active = f.key }, label = { Text(f.label) }) }
            }
            val field = fields.first { it.key == active }
            OutlinedTextField(
                value = values[field.key] ?: "",
                onValueChange = { set(field.key, it) },
                label = { Text(field.label) },
                supportingText = { Text(wordingError(field.key, values[field.key] ?: "") ?: cleared ?: field.hint) },
                isError = wordingError(field.key, values[field.key] ?: "") != null,
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { apply() }),
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { set(field.key, it) }
            if (item.kind == "dimension") {
                TextButton(
                    onClick = { vm.removeDimension(item) },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) {
                    Icon(Icons.Outlined.Delete, null)
                    Text("Remove from the drawing", Modifier.padding(start = 8.dp))
                }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}

/** The dimensions removed from the drawing, each with a button that puts it back. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RemovedSheet(vm: AppViewModel, doc: Doc) {
    ModalBottomSheet(onDismissRequest = { vm.showRemoved = false }) {
        Text(
            "Removed dimensions · ${doc.removed.size}",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
        )
        LazyColumn(Modifier.fillMaxWidth()) {
            items(doc.removed, key = { it.id }) { r ->
                ListItem(
                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                    headlineContent = { Text(r.label) },
                    trailingContent = {
                        FilledTonalButton(enabled = vm.busy == null, onClick = { vm.restoreDimension(r.id) }) {
                            Text("Restore")
                        }
                    },
                )
            }
            item { Spacer(Modifier.height(24.dp)) }
        }
    }
}

/**
 * The title block: every field, the one tapped first. A field the user has not filled shows
 * what the drawing prints now (from the model, or the core's default).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TitleSheet(vm: AppViewModel, doc: Doc, startKey: String) {
    val fields = remember { titleFields() }
    val authored = doc.title.associate { it.key to (it.authored ?: "") }
    val drawn = doc.title.associate { it.key to it.drawn }
    val values = remember { mutableStateMapOf<String, String>().apply { putAll(authored) } }
    var active by remember { mutableStateOf(fields.firstOrNull { it.key == startKey }?.key ?: fields.first().key) }
    val apply = {
        val changes = fields.mapNotNull { f ->
            val now = (values[f.key] ?: "").trim()
            if (now != (authored[f.key] ?: "").trim()) TitleChange(f.key, now) else null
        }
        if (changes.isEmpty()) vm.selected = null else vm.titles(changes)
    }
    ModalBottomSheet(onDismissRequest = { vm.selected = null }) {
        Column(Modifier.padding(horizontal = 24.dp).imePadding(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            EditorHeader("Title block", onCancel = { vm.selected = null }, onApply = apply)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                fields.forEach { f ->
                    // A dot on the fields the user has filled.
                    val filled = (values[f.key] ?: "").isNotBlank()
                    FilterChip(selected = active == f.key, onClick = { active = f.key }, label = { Text(if (filled) "${f.label} •" else f.label) })
                }
            }
            val field = fields.first { it.key == active }
            val text = values[field.key] ?: ""
            val now = drawn[field.key]?.takeIf { it.isNotBlank() && text.isBlank() }
            OutlinedTextField(
                value = text,
                onValueChange = { values[field.key] = it },
                label = { Text(field.label) },
                placeholder = { now?.let { Text(it) } },
                supportingText = {
                    Text(
                        when {
                            unsupportedText(text) -> "Latin letters only: the drawing font has no others."
                            now != null -> "The drawing shows \"$now\". ${field.hint}"
                            else -> field.hint
                        },
                    )
                },
                isError = unsupportedText(text),
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { apply() }),
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { values[field.key] = it }
            Spacer(Modifier.height(16.dp))
        }
    }
}
