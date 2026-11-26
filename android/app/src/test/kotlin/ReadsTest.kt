import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.core.app.ApplicationProvider
import android.content.Context
import com.trulinc.board.BoardScreen
import com.trulinc.board.BoardViewModel
import java.io.File
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.SocketPolicy
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** AC-5: reads carry the service token, and a failed read says why and keeps the board. */
@RunWith(RobolectricTestRunner::class)
class ReadsTest {
    @get:Rule val compose = createComposeRule()

    private val snapshot = """{"generated_at": 1000, "sources": [{"name": "linear", "last_good": 1000, "last_error": null}],
        "columns": [{"stage": "Build", "tickets": []}]}"""
    private val server = MockWebServer().apply { start() }
    private val context = ApplicationProvider.getApplicationContext<Context>()
    private var now = 0L
    private val vm = BoardViewModel(File(context.noBackupFilesDir, "settings.json"), clock = { now }).apply {
        // Plain http is allowed to this device only.
        assertNull(save(server.url("/").toString(), "id.access", "s3cret"))
    }

    @After
    fun stop() = server.shutdown()

    private fun read() = runBlocking {
        now += 60_000
        vm.read(asked = false)
        vm.view.value!!
    }

    @Test
    fun aReadSendsTheTokenAndTheAndroidUserAgent() {
        server.enqueue(MockResponse().setBody(snapshot))
        assertEquals(listOf("Build (0)"), read().board!!.columns.map { it.title })
        val request = server.takeRequest(5, TimeUnit.SECONDS)!!
        assertEquals("/v1/snapshot", request.path)
        assertEquals("id.access", request.getHeader("CF-Access-Client-Id"))
        assertEquals("s3cret", request.getHeader("CF-Access-Client-Secret"))
        assertTrue(request.getHeader("User-Agent")!!.startsWith("sdlc-board-android/"))
    }

    @Test
    fun eachFailureShowsItsCauseAndKeepsTheBoard() {
        server.enqueue(MockResponse().setBody(snapshot))
        read()
        server.enqueue(MockResponse().setResponseCode(401))
        assertEquals("Cloudflare Access or the board refused the service token", read().problem)
        server.enqueue(MockResponse().setSocketPolicy(SocketPolicy.NO_RESPONSE))
        val timedOut = read()
        assertEquals("no answer in time", timedOut.problem)
        assertNotNull(timedOut.board)

        server.shutdown()
        val unreachable = read()
        assertTrue(unreachable.problem!!, unreachable.problem!!.startsWith("couldn't reach the board"))
        compose.setContent { BoardScreen(unreachable, {}, {}) }
        compose.onNodeWithText("Couldn't refresh from ${server.url("/")}: ${unreachable.problem}").assertExists()
        compose.onNodeWithText("Build (0)").assertExists()
    }
}
