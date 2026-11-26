import com.sun.net.httpserver.HttpServer
import com.trulinc.board.core.Board
import com.trulinc.board.core.Settings
import com.trulinc.board.core.coreVersion
import com.trulinc.board.core.settingsProblem
import com.trulinc.board.core.settingsTrimmed
import java.net.InetSocketAddress
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The bindings on the JVM, against the host build of board-ffi: a recorded
 * snapshot goes through the core's read, keep and wording, as on the phone.
 */
class BoardTest {
    private val snapshot = """
        {"generated_at": 1000, "sources": [{"name": "linear", "last_good": 1000, "last_error": null}],
         "columns": [{"stage": "Spec", "tickets": []}, {"stage": "Build", "tickets": [
          {"id": "TRU-8", "title": "One board", "url": "u", "repo": "r", "stage": "Build", "children": [
            {"id": "TRU-9", "title": "a", "url": "u", "repo": "r", "stage": "Verify", "children": [
              {"id": "TRU-10", "title": "b", "url": "u", "repo": "r", "stage": "Plan", "children": []}]}]}]}]}
    """.trimIndent()

    // What the server answers next: a status and a body.
    private val answer = AtomicReference(200 to snapshot)
    private val userAgent = AtomicReference<String?>()
    private val accessId = AtomicReference<String?>()
    private val reads = AtomicInteger()
    private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
        createContext("/v1/snapshot") { ex ->
            reads.incrementAndGet()
            userAgent.set(ex.requestHeaders.getFirst("User-Agent"))
            accessId.set(ex.requestHeaders.getFirst("CF-Access-Client-Id"))
            val (status, body) = answer.get()
            val bytes = body.toByteArray()
            ex.sendResponseHeaders(status, bytes.size.toLong())
            ex.responseBody.use { it.write(bytes) }
        }
        start()
    }
    private val url = "http://127.0.0.1:${server.address.port}"

    @After
    fun stop() = server.stop(0)

    @Test
    fun aRecordedSnapshotReadsIntoColumnsWithNestedSubTickets() {
        val board = Board(Settings(url = " $url ", clientId = "id.access", clientSecret = "s3cret"))
        assertTrue(board.refresh(false, 0uL))
        assertTrue(userAgent.get()!!.startsWith("sdlc-board-android/"))
        assertEquals("id.access", accessId.get())

        val view = board.view(0uL).board!!
        assertEquals(listOf("Spec (0)", "Build (1)"), view.columns.map { it.title })
        val card = view.columns[1].cards.single()
        assertEquals("TRU-8", card.id)
        assertEquals(
            listOf(Triple(0u, "TRU-9", "· Verify"), Triple(1u, "TRU-10", "· Plan")),
            card.subTickets.map { Triple(it.depth, it.id, it.stage) },
        )
        assertTrue(view.refreshed.startsWith("Last refreshed: linear "))
        assertNull(view.needsYouHeading)
    }

    @Test
    fun aReadIsOnlyDueAfterTheRefreshIntervalUnlessAsked() {
        val board = Board(Settings(url, "", ""))
        assertTrue(board.refresh(false, 0uL))
        assertEquals(false, board.refresh(false, 29_000uL))
        assertTrue(board.refresh(true, 29_000uL))
        assertTrue(board.refresh(false, 59_000uL))
        assertEquals(3, reads.get())
    }

    // The app's clock counts the time the phone slept, so a night asleep shows.
    @Test
    fun aLongSleepMakesTheBoardOldAndDueAgain() {
        val board = Board(Settings(url, "", ""))
        board.refresh(false, 1_000uL)
        val night = 2_000uL + 8uL * 3_600_000uL
        assertEquals("Last refreshed: linear 8 h ago", board.view(night).board!!.refreshed)
        assertTrue(board.refresh(false, night))
        assertEquals("Last refreshed: linear 0 s ago", board.view(night).board!!.refreshed)
    }

    @Test
    fun aFailedReadKeepsTheLastBoardAndSaysWhy() {
        val board = Board(Settings(url, "", ""))
        board.refresh(false, 0uL)
        val before = board.view(0uL).board
        answer.set(401 to "")
        assertTrue(board.refresh(true, 0uL))
        val after = board.view(0uL)
        assertEquals(before!!.columns, after.board!!.columns)
        assertEquals("the board needs an Access service token (it answered 401)", after.problem)
        server.stop(0)
        board.refresh(true, 0uL)
        assertTrue(board.view(0uL).problem!!.startsWith("couldn't reach the board"))
        assertNotNull(board.view(0uL).board)
    }

    @Test
    fun settingsAreTrimmedAndAHalfSetTokenIsRefused() {
        val pasted = Settings(" https://board.example\n", " id.access ", "s3cret ")
        assertEquals(Settings("https://board.example", "id.access", "s3cret"), settingsTrimmed(pasted))
        assertEquals(
            "set both the Access client ID and secret, or neither",
            settingsProblem(Settings("https://board.example", "id.access", "")),
        )
        assertEquals("set the board's address", settingsProblem(Settings("", "", "")))
        assertNull(settingsProblem(Settings("https://board.example", "", "")))
    }

    @Test
    fun theCoreSaysItsVersion() {
        assertTrue(coreVersion().matches(Regex("""\d+\.\d+\.\d+""")))
    }
}
