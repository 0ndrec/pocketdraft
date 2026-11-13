package app.pocketdraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.pocketdraft.AppViewModel
import uniffi.draftwright_ffi.TitleChange
import uniffi.draftwright_ffi.titleDefaults

/**
 * The app's settings, over whatever screen opened them: how sheets are laid out, what the
 * title block credits, and the title-block values new drawings start with. Switches apply at
 * once; the defaults are stored on leaving.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(vm: AppViewModel) {
    val close = { vm.showSettings = false }
    BackHandler(onBack = close)
    val defaults = remember { titleDefaults() }
    val values = remember { mutableStateMapOf<String, String>().apply { defaults.forEach { put(it.field.key, it.value) } } }
    DisposableEffect(Unit) {
        onDispose {
            val changed = defaults.any { (values[it.field.key] ?: "").trim() != it.value.trim() }
            if (changed) vm.saveDefaults(values.map { TitleChange(it.key, it.value) })
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = { IconButton(onClick = close) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") } },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().imePadding().verticalScroll(rememberScrollState())) {
            Heading("Sheet")
            Toggle(
                "Keep paper and scale",
                "Edits stay on the paper and scale a drawing has, which is much faster for large parts. Choose Automatic in the paper menu to lay a drawing out again.",
                vm.keepSheet,
            ) { vm.toggleKeepSheet() }
            Toggle(
                "Exact size",
                "A paper you choose is kept even when the drawing does not fit it.",
                vm.exact,
            ) { vm.toggleExact() }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading("Title block")
            Toggle(
                "Credit draftwright in Drawn by",
                "Off, the Drawn by cell shows the author alone, or stays empty.",
                vm.credit,
            ) { vm.toggleCredit() }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading("New drawings start with")
            defaults.forEach { d ->
                OutlinedTextField(
                    value = values[d.field.key] ?: "",
                    onValueChange = { values[d.field.key] = it },
                    label = { Text(d.field.label) },
                    supportingText = { Text(d.field.hint) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
        }
    }
}

@Composable
private fun Heading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

/** A setting with a switch; the whole row toggles it. */
@Composable
private fun Toggle(title: String, about: String, on: Boolean, toggle: () -> Unit) {
    ListItem(
        modifier = Modifier.clickableRow(toggle),
        headlineContent = { Text(title) },
        supportingContent = { Text(about) },
        trailingContent = { Switch(checked = on, onCheckedChange = { toggle() }) },
    )
}
