package app.pocketdraft.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import app.pocketdraft.AppViewModel
import uniffi.draftwright_ffi.Doc

/** The sides of the part a datum can sit on: the core's name and the one shown. */
private val SIDES = listOf(
    "bottom" to "Bottom",
    "top" to "Top",
    "left" to "Left",
    "right" to "Right",
    "front" to "Front",
    "back" to "Back",
)
private val LETTERS = listOf("A", "B", "C", "D", "E", "F")

/** A part axis with the sides at its two ends (`min` first). */
private class Axis(val key: String, val label: String, val min: String, val max: String)

private val AXES = listOf(
    Axis("x", "Along the width", "left", "right"),
    Axis("y", "Along the depth", "front", "back"),
    Axis("z", "Along the height", "bottom", "top"),
)

private fun sideName(side: String) = SIDES.first { it.first == side }.second

/**
 * Datums and origins: which sides of the part are datums (A, B, C…), and which end of the
 * part its dimensions are measured from. Every choice is applied at once, as one undo step;
 * the controls wait while the drawing is laid out again.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DatumSheet(vm: AppViewModel, doc: Doc) {
    val idle = vm.busy == null
    val rows = ListItemDefaults.colors(containerColor = Color.Transparent)
    ModalBottomSheet(
        onDismissRequest = { vm.showDatums = false },
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Text(
                "Datums and origins",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            Box(Modifier.fillMaxWidth().padding(top = 8.dp).height(4.dp)) {
                if (!idle) LinearProgressIndicator(Modifier.fillMaxWidth())
            }

            Heading("Datum faces")
            Note("A datum is a face that dimensions and tolerances refer to. Dimensions are measured from datum faces.")
            SIDES.forEach { (side, name) ->
                ListItem(
                    colors = rows,
                    headlineContent = { Text(name) },
                    trailingContent = { LetterMenu(vm, doc, side, idle) },
                )
            }

            HorizontalDivider(Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
            Heading("Measure dimensions from")
            AXES.forEach { axis -> Origin(vm, doc, axis, idle) }

            HorizontalDivider(Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
            ListItem(
                colors = rows,
                headlineContent = { Text("Show every datum") },
                supportingContent = { Text("Also the datums that no dimension or tolerance refers to") },
                trailingContent = {
                    Switch(checked = doc.uncitedDatums, enabled = idle, onCheckedChange = { vm.uncitedDatums(it) })
                },
            )
        }
    }
}

@Composable
private fun Heading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
    )
}

@Composable
private fun Note(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 24.dp),
    )
}

/**
 * A side of the part tapped on the drawing (an edge in a view, or its datum symbol): its datum
 * letter, chosen at once. Unknown sides (an older page message) show nothing.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun SideSheet(vm: AppViewModel, doc: Doc, side: String) {
    val name = SIDES.firstOrNull { it.first == side }?.second ?: return
    val now = doc.datums[side]
    val idle = vm.busy == null
    ModalBottomSheet(onDismissRequest = { vm.selected = null }) {
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp)) {
            Text("$name face", style = MaterialTheme.typography.titleLarge)
            Text(
                if (now == null) "Not a datum. Choose a letter to make it one." else "Datum $now",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            FlowRow(
                Modifier.padding(top = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                (listOf<String?>(null) + LETTERS).forEach { letter ->
                    val holder = letter?.let { l -> doc.datums.entries.firstOrNull { it.value == l && it.key != side }?.key }
                    FilterChip(
                        selected = now == letter,
                        enabled = idle,
                        onClick = { if (now != letter) vm.datum(side, letter) },
                        label = {
                            Text(
                                when {
                                    letter == null -> "None"
                                    holder != null -> "$letter · on ${sideName(holder)}"
                                    else -> letter
                                },
                            )
                        },
                    )
                }
            }
            TextButton(
                modifier = Modifier.padding(top = 8.dp),
                onClick = { vm.selected = null; vm.showDatums = true },
            ) { Text("All datums and origins") }
        }
    }
}

/** The datum letter of a side, in a menu; a letter another side holds moves here. */
@Composable
private fun LetterMenu(vm: AppViewModel, doc: Doc, side: String, idle: Boolean) {
    var open by remember { mutableStateOf(false) }
    val now = doc.datums[side]
    Box {
        OutlinedButton(enabled = idle, onClick = { open = true }) {
            Text(now ?: "None")
            Icon(Icons.Filled.ArrowDropDown, null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("None") },
                leadingIcon = { if (now == null) Icon(Icons.Filled.Check, null) },
                onClick = { open = false; if (now != null) vm.datum(side, null) },
            )
            LETTERS.forEach { letter ->
                val holder = doc.datums.entries.firstOrNull { it.value == letter && it.key != side }?.key
                DropdownMenuItem(
                    text = { Text(letter) },
                    leadingIcon = { if (now == letter) Icon(Icons.Filled.Check, null) },
                    trailingIcon = {
                        holder?.let {
                            Text("on ${sideName(it)}", color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                    onClick = { open = false; if (now != letter) vm.datum(side, letter) },
                )
            }
        }
    }
}

/** Which end of the part the dimensions along one axis start from. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Origin(vm: AppViewModel, doc: Doc, axis: Axis, idle: Boolean) {
    val now = doc.origins[axis.key]
    val choices = listOf(null to "Auto", "min" to sideName(axis.min), "max" to sideName(axis.max))
    Column(Modifier.padding(horizontal = 24.dp, vertical = 8.dp)) {
        Text(axis.label, style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.height(8.dp))
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            choices.forEachIndexed { i, (end, label) ->
                SegmentedButton(
                    selected = now == end,
                    enabled = idle,
                    onClick = { if (now != end) vm.origin(axis.key, end) },
                    shape = SegmentedButtonDefaults.itemShape(i, choices.size),
                ) { Text(label) }
            }
        }
        if (now == null) {
            // As the core chooses: the datum face across the axis (the first letter), else the minimum.
            val datum = listOf(axis.min, axis.max)
                .mapNotNull { side -> doc.datums[side]?.let { it to side } }
                .minByOrNull { it.first }
            Text(
                if (datum != null) "Auto: from datum ${datum.first} (${sideName(datum.second)})"
                else "Auto: from the ${sideName(axis.min).lowercase()}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }
}
