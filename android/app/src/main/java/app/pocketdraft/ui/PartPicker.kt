package app.pocketdraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.ViewInAr
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.pocketdraft.AppViewModel
import app.pocketdraft.Picking

/** More parts than this and the list gets a search field. */
private const val SEARCH_FROM = 8

/**
 * What to draw from an assembly: all of it, or one part. A modal bottom sheet (the Material 3
 * container for a list of choices on a phone); swiping it down or tapping outside cancels.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PartPicker(vm: AppViewModel, picking: Picking) {
    var search by remember(picking) { mutableStateOf("") }
    val shown = remember(picking, search) {
        val wanted = search.trim()
        picking.parts.filter { it.name.contains(wanted, ignoreCase = true) }
    }
    val faces = picking.parts.sumOf { it.faces * it.instances }
    val rows = ListItemDefaults.colors(containerColor = Color.Transparent)
    ModalBottomSheet(
        onDismissRequest = vm::cancelPicking,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 8.dp)) {
            Text("What to draw?", style = MaterialTheme.typography.titleLarge)
            Text(
                "${picking.name} is an assembly of ${picking.parts.size} parts",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        LazyColumn(contentPadding = PaddingValues(bottom = 24.dp)) {
            item {
                ListItem(
                    colors = rows,
                    modifier = Modifier.clickableRow { vm.pick(null) },
                    leadingContent = { Badge(Icons.Outlined.AccountTree, strong = true) },
                    headlineContent = { Text("Whole assembly") },
                    supportingContent = { Text("All parts on one drawing · $faces faces") },
                )
                HorizontalDivider(Modifier.padding(horizontal = 24.dp, vertical = 8.dp))
                Text(
                    "One part",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                )
                if (picking.parts.size > SEARCH_FROM) {
                    OutlinedTextField(
                        value = search,
                        onValueChange = { search = it },
                        placeholder = { Text("Search parts") },
                        leadingIcon = { Icon(Icons.Filled.Search, null) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 4.dp),
                    )
                }
            }
            items(shown, key = { it.index.toInt() }) { part ->
                ListItem(
                    colors = rows,
                    modifier = Modifier.clickableRow { vm.pick(part) },
                    leadingContent = { Badge(Icons.Outlined.ViewInAr, strong = false) },
                    headlineContent = { Text(partTitle(part.name), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    supportingContent = {
                        val n = part.instances.toInt()
                        Text("${part.faces} faces" + if (n > 1) " · used $n times" else "")
                    },
                )
            }
            if (shown.isEmpty()) {
                item {
                    Text(
                        "No part matches \"${search.trim()}\"",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                    )
                }
            }
        }
    }
}

/** A part name without the file extension CAD exports leave on it. */
private fun partTitle(name: String): String {
    val dot = name.lastIndexOf('.')
    val known = dot > 0 && name.substring(dot + 1).lowercase() in setOf("stp", "step", "sldprt", "ipt", "prt")
    return if (known) name.substring(0, dot) else name
}

/** The round icon that leads a row; `strong` for the choice offered first. */
@Composable
private fun Badge(icon: ImageVector, strong: Boolean) {
    val scheme = MaterialTheme.colorScheme
    Box(
        Modifier.size(40.dp).background(if (strong) scheme.primaryContainer else scheme.secondaryContainer, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = if (strong) scheme.onPrimaryContainer else scheme.onSecondaryContainer)
    }
}
