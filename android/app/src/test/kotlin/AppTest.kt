import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import com.trulinc.board.MainActivity
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** AC-2 in the app itself: a fresh install asks first, keeps the settings outside backups, and Settings changes them. */
@RunWith(RobolectricTestRunner::class)
class AppTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun aFreshInstallAsksForTheSettingsAndSettingsChangesThem() {
        // No ticking: nothing here should read the network.
        compose.mainClock.autoAdvance = false
        compose.onNodeWithText("Board settings").assertExists()
        compose.onNodeWithText("Cancel").assertDoesNotExist()
        compose.onNodeWithTag("url").performTextClearance()
        compose.onNodeWithTag("url").performTextInput("https://board.example")
        compose.onNodeWithText("Save").performClick()

        val noBackup = File(compose.activity.noBackupFilesDir, "settings.json")
        assertTrue(noBackup.readText().contains("https://board.example"))
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("https://board.example").assertExists()
        compose.onNodeWithText("Cancel").performClick()
        compose.onNodeWithText("Board settings").assertDoesNotExist()
    }
}
