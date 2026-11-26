package com.trulinc.board

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import com.trulinc.board.core.coreVersion

/**
 * For now, only proof that the Rust core loads on the phone: the settings
 * screen (TRU-30) and the board (TRU-31) come next.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val version = coreVersion()
        setContent {
            MaterialTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { insets ->
                    Text("SDLC board, core $version", modifier = Modifier.padding(insets))
                }
            }
        }
    }
}
