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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
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
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
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

/** The drawing's problems; each opens to its ready fixes or the value it asks for. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProblemsSheet(vm: AppViewModel, doc: Doc) {
    var expanded by remember { mutableStateOf<String?>(null) }
    // Opens at full height: an opened problem's field and fixes would lie below a half sheet.
    ModalBottomSheet(
        onDismissRequest = { vm.showProblems = false },
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        val fixable = doc.problems.count { it.fixes.isNotEmpty() || it.ask != null }
        Text(
            "Problems · ${doc.problemTotal} · $fixable fixable",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
        )
        LazyColumn(Modifier.fillMaxWidth().imePadding()) {
            items(doc.problems, key = { it.key }) { p ->
                ProblemRow(vm, p, expanded == p.key) { expanded = if (expanded == p.key) null else p.key }
            }
            item { Spacer(Modifier.height(24.dp)) }
        }
    }
}

@Composable
private fun ProblemRow(vm: AppViewModel, p: Problem, open: Boolean, toggle: () -> Unit) {
    val icon = when (p.severity) {
        "error" -> Icons.Filled.Error
        "warning" -> Icons.Filled.Warning
        else -> Icons.Filled.Info
    }
    ListItem(
        // Clear, so the row takes the tone of the sheet.
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        modifier = Modifier.clickableRow(toggle),
        leadingContent = {
            Icon(icon, null, tint = if (p.severity == "error") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
        },
        headlineContent = { Text(p.headline) },
        supportingContent = { Text(p.message, maxLines = if (open) Int.MAX_VALUE else 2) },
    )
    if (!open) return
    Column(Modifier.padding(start = 72.dp, end = 16.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        p.ask?.let { field ->
            var value by remember { mutableStateOf("") }
            OutlinedTextField(
                value = value,
                onValueChange = { value = it },
                label = { Text(field.label) },
                supportingText = { Text(field.hint) },
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { value = it }
            Button(enabled = value.isNotBlank(), onClick = { vm.answer(p.key, value) }) { Text("Apply") }
        }
        p.fixes.forEach { fix ->
            Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
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

/** A dimension or label picked on the drawing: tolerance, fit, text before and after. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ItemSheet(vm: AppViewModel, item: Item) {
    val fields = remember(item.kind) { itemFields(item.kind) }
    val start = mapOf(
        "dim:tolerance" to (item.tolerance ?: ""),
        "dim:fit" to (item.fit ?: ""),
        "dim:prefix" to (item.prefix ?: "").trim(),
        "dim:suffix" to (item.suffix ?: "").trim(),
    )
    val values = remember(item.id) { mutableStateMapOf<String, String>().apply { putAll(start) } }
    var active by remember(item.id) { mutableStateOf(fields.first().key) }
    ModalBottomSheet(onDismissRequest = { vm.selected = null }) {
        Column(Modifier.padding(horizontal = 24.dp).imePadding(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(item.title, style = MaterialTheme.typography.titleLarge)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                fields.forEach { f -> FilterChip(selected = active == f.key, onClick = { active = f.key }, label = { Text(f.label) }) }
            }
            val field = fields.first { it.key == active }
            OutlinedTextField(
                value = values[field.key] ?: "",
                onValueChange = { values[field.key] = it },
                label = { Text(field.label) },
                supportingText = { Text(field.hint) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { values[field.key] = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                if (item.kind == "dimension") {
                    TextButton(
                        onClick = { vm.removeDimension(item) },
                        colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                    ) {
                        Icon(Icons.Outlined.Delete, null)
                        Text("Remove", Modifier.padding(start = 8.dp))
                    }
                }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = { vm.selected = null }) { Text("Cancel") }
                Button(onClick = {
                    vm.edit(
                        item,
                        values["dim:tolerance"] ?: "",
                        values["dim:fit"] ?: "",
                        values["dim:prefix"] ?: "",
                        values["dim:suffix"] ?: "",
                    )
                }) { Text("Apply") }
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

/** The title block: every field, the one tapped first. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TitleSheet(vm: AppViewModel, doc: Doc, startKey: String) {
    val fields = remember { titleFields() }
    val authored = doc.title.associate { it.key to (it.authored ?: "") }
    val drawn = doc.title.associate { it.key to it.drawn }
    val values = remember { mutableStateMapOf<String, String>().apply { putAll(authored) } }
    var active by remember { mutableStateOf(fields.firstOrNull { it.key == startKey }?.key ?: fields.first().key) }
    ModalBottomSheet(onDismissRequest = { vm.selected = null }) {
        Column(Modifier.padding(horizontal = 24.dp).imePadding(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Title block", style = MaterialTheme.typography.titleLarge)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                fields.forEach { f -> FilterChip(selected = active == f.key, onClick = { active = f.key }, label = { Text(f.label) }) }
            }
            val field = fields.first { it.key == active }
            val text = values[field.key] ?: ""
            val auto = drawn[field.key]?.takeIf { (authored[field.key] ?: "").isEmpty() }
            OutlinedTextField(
                value = text,
                onValueChange = { values[field.key] = it },
                label = { Text(field.label) },
                placeholder = { auto?.let { Text("Auto: $it") } },
                supportingText = {
                    Text(if (unsupportedText(text)) "Latin letters only: the drawing font has no others." else field.hint)
                },
                isError = unsupportedText(text),
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Suggestions(field) { values[field.key] = it }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = { vm.selected = null }) { Text("Cancel") }
                Button(onClick = {
                    val changes = fields.mapNotNull { f ->
                        val now = (values[f.key] ?: "").trim()
                        if (now != (authored[f.key] ?: "").trim()) TitleChange(f.key, now) else null
                    }
                    if (changes.isEmpty()) vm.selected = null else vm.titles(changes)
                }) { Text("Apply") }
            }
            Spacer(Modifier.height(16.dp))
        }
    }
}
