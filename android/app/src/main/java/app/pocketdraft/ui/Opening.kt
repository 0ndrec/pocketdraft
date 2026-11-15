package app.pocketdraft.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import uniffi.draftwright_ffi.Progress

/**
 * The stage a model being opened has reached, what it holds once read, and an estimated bar
 * for the stage (the core reports no progress of its own: the share is an estimate from how
 * long earlier opens took on this phone).
 */
@Composable
fun OpeningCard(p: Progress, modifier: Modifier = Modifier) {
    Card(
        modifier = modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(
                    "Step ${p.stage} of ${p.stages}",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    seconds(p.elapsedMs),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(p.label, style = MaterialTheme.typography.titleMedium)
            if (p.detail.isNotEmpty()) {
                Text(p.detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            val fraction = p.fraction
            if (fraction == null) {
                LinearProgressIndicator(Modifier.fillMaxWidth())
            } else {
                // Polled four times a second: ease between the readings.
                val shown by animateFloatAsState(fraction, tween(250), label = "stage")
                LinearProgressIndicator(
                    progress = { shown },
                    modifier = Modifier.fillMaxWidth().semantics { contentDescription = "About ${(fraction * 100).toInt()} % of this step" },
                )
            }
        }
    }
}

private fun seconds(ms: ULong): String {
    val s = (ms / 1000u).toLong()
    return if (s < 60) "$s s" else "${s / 60} min ${s % 60} s"
}
