import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import com.trulinc.board.BoardViewModel
import com.trulinc.board.SettingsScreen
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** AC-2: the settings take typed and pasted text, and are saved trimmed where backups don't reach. */
@RunWith(RobolectricTestRunner::class)
class SettingsTest {
    @get:Rule val compose = createComposeRule()

    private val context = ApplicationProvider.getApplicationContext<Context>()
    private val file = File(context.noBackupFilesDir, "settings.json")
    private val vm = BoardViewModel(file)

    private fun paste(tag: String, text: String) {
        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("pasted", text))
        compose.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.PasteText)
    }

    private fun show() = compose.setContent { SettingsScreen(vm.settings.value, vm::save, onCancel = null) }

    @Test
    fun typedAndPastedSettingsAreSavedTrimmedOutsideBackups() {
        assertNull(vm.settings.value)
        show()
        compose.onNodeWithTag("url").performTextClearance()
        compose.onNodeWithTag("url").performTextInput(" https://board.example ")
        paste("clientId", "id.access\n")
        paste("secret", "  s3cret ")
        compose.onNodeWithText("Save").performClick()

        assertEquals(File(context.noBackupFilesDir, "settings.json"), file)
        assertEquals(
            """{"url":"https://board.example","client_id":"id.access","client_secret":"s3cret"}""",
            file.readText(),
        )
        assertEquals("s3cret", BoardViewModel(file).settings.value!!.clientSecret)
    }

    @Test
    fun saveRefusesAnEmptyAddressAndAHalfSetToken() {
        show()
        compose.onNodeWithTag("clientId").performTextInput("id.access")
        compose.onNodeWithText("Save").performClick()
        compose.onNodeWithText("set both the Access client ID and secret, or neither").assertExists()

        compose.onNodeWithTag("url").performTextClearance()
        compose.onNodeWithText("Save").performClick()
        compose.onNodeWithText("set the board's address").assertExists()
        assertFalse(file.exists())
    }
}
