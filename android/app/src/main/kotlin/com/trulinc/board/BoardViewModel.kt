package com.trulinc.board

import android.os.SystemClock
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.trulinc.board.core.Board
import com.trulinc.board.core.Settings
import com.trulinc.board.core.View
import com.trulinc.board.core.settingsLoad
import com.trulinc.board.core.settingsSave
import com.trulinc.board.core.settingsTrimmed
import java.io.File
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The settings, kept in [settingsFile], and the board read with them. The
 * core decides when to read and what to keep; this only runs it off the
 * main thread. Never log [settings]: its toString shows the secret.
 */
class BoardViewModel(
    private val settingsFile: File,
    // Counts the time the phone sleeps, unlike the core's own clock.
    private val clock: () -> Long = SystemClock::elapsedRealtime,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val _settings = MutableStateFlow(settingsLoad(settingsFile.path))
    /** Null until the first save: the app opens the settings then. */
    val settings: StateFlow<Settings?> = _settings

    private var board = _settings.value?.let(::Board)
    private val _view = MutableStateFlow<View?>(null)
    val view: StateFlow<View?> = _view

    /** Reads in the background if a read is due, or now if [asked]. */
    fun tick(asked: Boolean) {
        viewModelScope.launch { read(asked) }
    }

    /** Reads if due, or now if [asked], then shows the board as it stands. */
    suspend fun read(asked: Boolean) {
        val board = board ?: return
        show(board)
        withContext(io) { board.refresh(asked, now()) }
        show(board)
    }

    /** Saves the settings for the board to read with, or says why it can't. */
    fun save(url: String, clientId: String, clientSecret: String): String? {
        val settings = Settings(url, clientId, clientSecret)
        settingsSave(settings, settingsFile.path)?.let { return it }
        _settings.value = settingsTrimmed(settings)
        // The next tick reads with them.
        board = Board(settingsTrimmed(settings))
        _view.value = null
        return null
    }

    private fun show(board: Board) {
        if (board === this.board) _view.value = board.view(now())
    }

    private fun now() = clock().toULong()
}
