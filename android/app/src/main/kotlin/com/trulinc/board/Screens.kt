package com.trulinc.board

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import com.trulinc.board.core.Settings
import com.trulinc.board.core.View

/**
 * The board's address and Access service token. Each field takes a paste
 * from the clipboard. [onSave] answers why it couldn't save, or null;
 * [onCancel] is null on first start, when there's nothing to go back to.
 */
@Composable
fun SettingsScreen(
    saved: Settings?,
    onSave: (url: String, clientId: String, clientSecret: String) -> String?,
    onCancel: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    var url by rememberSaveable { mutableStateOf(saved?.url ?: "https://board.trulinc.com") }
    var clientId by rememberSaveable { mutableStateOf(saved?.clientId ?: "") }
    // Not saveable: a saved-state Bundle leaves the app's own storage.
    var secret by remember { mutableStateOf(saved?.clientSecret ?: "") }
    var problem by rememberSaveable { mutableStateOf<String?>(null) }
    Column(
        modifier.verticalScroll(rememberScrollState()).imePadding().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Board settings", style = MaterialTheme.typography.titleLarge)
        val field = Modifier.fillMaxWidth()
        OutlinedTextField(url, { url = it }, field.testTag("url"), label = { Text("Board address") },
            singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri))
        OutlinedTextField(clientId, { clientId = it }, field.testTag("clientId"),
            label = { Text("Access client ID") }, singleLine = true)
        OutlinedTextField(secret, { secret = it }, field.testTag("secret"), label = { Text("Access client secret") },
            singleLine = true, visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password))
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button({ problem = onSave(url, clientId, secret) }) { Text("Save") }
            onCancel?.let { OutlinedButton(it) { Text("Cancel") } }
        }
    }
}

/**
 * Why the board may be out of date, above the last board read. The board
 * itself is laid out in TRU-31; for now, its refresh line and columns.
 */
@Composable
fun BoardScreen(view: View?, onRefresh: () -> Unit, onSettings: () -> Unit, modifier: Modifier = Modifier) {
    Column(modifier.verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Button(onRefresh, enabled = view?.waiting != true) { Text(if (view?.waiting == true) "Reading…" else "Refresh") }
            OutlinedButton(onSettings) { Text("Settings") }
        }
        view?.problemLine?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        view?.placeholder?.let { Text(it) }
        val board = view?.board
        if (board != null) {
            Text(board.refreshed, style = MaterialTheme.typography.bodySmall)
            board.columns.forEach { Text(it.title) }
        }
    }
}
