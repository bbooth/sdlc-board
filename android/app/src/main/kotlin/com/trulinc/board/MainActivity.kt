package com.trulinc.board

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.io.File
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // noBackupFilesDir: Auto Backup never copies it, and the manifest turns backups off too.
        val settingsFile = File(noBackupFilesDir, "settings.json")
        setContent {
            val vm: BoardViewModel = viewModel(factory = viewModelFactory { initializer { BoardViewModel(settingsFile) } })
            val settings by vm.settings.collectAsStateWithLifecycle()
            val view by vm.view.collectAsStateWithLifecycle()
            var editing by rememberSaveable { mutableStateOf(false) }
            // Once a second: tick the ages, and read when the core says one is due.
            LaunchedEffect(vm) {
                while (true) {
                    vm.tick(false)
                    delay(1_000)
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
                        BoardScreen(view, saved.url, { vm.tick(true) }, { editing = true }, Modifier.padding(insets))
                    }
                }
            }
        }
    }
}
