package com.trulinc.board

import android.content.Context
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.io.File
import kotlinx.coroutines.delay

/** Where the settings live: Auto Backup never copies `noBackupFilesDir`, and the manifest turns backups off too. */
fun settingsFile(context: Context) = File(context.noBackupFilesDir, "settings.json")

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Dark icons: the app is drawn light whatever the phone's own theme.
        val bars = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
        enableEdgeToEdge(statusBarStyle = bars, navigationBarStyle = bars)
        val file = settingsFile(this)
        setContent {
            BoardApp(viewModel(factory = viewModelFactory { initializer { BoardViewModel(file) } }))
        }
    }
}

/** The settings on first start, else the board, with Settings to change them. */
@Composable
fun BoardApp(vm: BoardViewModel) {
    val settings by vm.settings.collectAsStateWithLifecycle()
    val view by vm.view.collectAsStateWithLifecycle()
    var editing by rememberSaveable { mutableStateOf(false) }
    // Once a second while the app is in view: tick the ages, and read when the core says one is due.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(vm) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                vm.tick(false)
                delay(1_000)
            }
        }
    }
    MaterialTheme {
        Scaffold(Modifier.fillMaxSize()) { insets ->
            val saved = settings
            if (saved == null || editing) {
                SettingsScreen(
                    saved,
                    onSave = { u, i, s -> vm.save(u, i, s).also { if (it == null) editing = false } },
                    onCancel = saved?.let { { editing = false } },
                    modifier = Modifier.padding(insets),
                )
            } else {
                BoardScreen(view, { vm.tick(true) }, { editing = true }, Modifier.padding(insets))
            }
        }
    }
}
