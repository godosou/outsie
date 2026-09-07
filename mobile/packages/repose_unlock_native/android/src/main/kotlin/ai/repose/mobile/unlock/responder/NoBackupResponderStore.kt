package ai.repose.mobile.unlock.responder

import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import java.io.Closeable
import java.io.File
import java.nio.file.Files

internal class NoBackupResponderStore(
    context: Context,
    databaseName: String = DATABASE_NAME,
) : ResponderStore, Closeable {
    private val applicationContext = context.applicationContext
    private val monitor = Any()
    private var openedDatabase: SQLiteDatabase? = null

    internal val databaseFile: File

    init {
        if (!DATABASE_NAME_PATTERN.matches(databaseName)) throw ResponderStoreException()
        val noBackupDirectory = applicationContext.noBackupFilesDir
        databaseFile = noBackupDirectory.resolve(databaseName)
        if (databaseFile.parentFile?.canonicalFile != noBackupDirectory.canonicalFile) {
            throw ResponderStoreException()
        }
    }

    override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord? =
        synchronized(monitor) {
            try {
                loadEncoded(database(), recordKey(macId, deviceId))?.let { encoded ->
                    ResponderStateCodec.decode(encoded).also { decoded ->
                        if (!decoded.macId.contentEquals(macId) ||
                            !decoded.deviceId.contentEquals(deviceId)
                        ) {
                            throw ResponderStoreException()
                        }
                    }
                }
            } catch (exception: ResponderStoreException) {
                throw exception
            } catch (_: Exception) {
                throw ResponderStoreException()
            }
        }

    override fun compareAndSwap(
        macId: ByteArray,
        deviceId: ByteArray,
        expected: DurableResponderRecord?,
        replacement: DurableResponderRecord,
    ): Boolean = synchronized(monitor) {
        if (!replacement.macId.contentEquals(macId) ||
            !replacement.deviceId.contentEquals(deviceId)
        ) {
            throw ResponderStoreException()
        }
        val expectedBytes = expected?.let { ResponderStateCodec.encode(expected) }
        val replacementBytes = ResponderStateCodec.encode(replacement)
        val key = recordKey(macId, deviceId)
        val sqlite = database()
        try {
            sqlite.beginTransaction()
            val currentBytes = loadEncoded(sqlite, key)
            if (!sameNullableBytes(currentBytes, expectedBytes)) {
                sqlite.setTransactionSuccessful()
                return@synchronized false
            }
            val values = ContentValues(2).apply {
                put(COLUMN_KEY, key)
                put(COLUMN_STATE, replacementBytes)
            }
            if (sqlite.insertWithOnConflict(
                    TABLE_NAME,
                    null,
                    values,
                    SQLiteDatabase.CONFLICT_REPLACE,
                ) == -1L
            ) {
                throw ResponderStoreException()
            }
            sqlite.setTransactionSuccessful()
        } catch (exception: ResponderStoreException) {
            throw exception
        } catch (_: Exception) {
            throw ResponderStoreException()
        } finally {
            if (sqlite.inTransaction()) {
                try {
                    sqlite.endTransaction()
                } catch (_: Exception) {
                    throw ResponderStoreException()
                }
            }
        }
        true
    }

    override fun close() {
        synchronized(monitor) {
            try {
                openedDatabase?.close()
            } finally {
                openedDatabase = null
            }
        }
    }

    private fun database(): SQLiteDatabase {
        openedDatabase?.let { database ->
            if (database.isOpen) return database
        }
        if (databaseFile.exists() && Files.isSymbolicLink(databaseFile.toPath())) {
            throw ResponderStoreException()
        }
        val sqlite = try {
            SQLiteDatabase.openOrCreateDatabase(databaseFile, null)
        } catch (_: Exception) {
            throw ResponderStoreException()
        }
        try {
            sqlite.execSQL("PRAGMA synchronous=FULL")
            sqlite.beginTransaction()
            when (sqlite.version) {
                0 -> {
                    sqlite.execSQL(CREATE_TABLE)
                    sqlite.version = SCHEMA_VERSION
                }
                SCHEMA_VERSION -> sqlite.execSQL(CREATE_TABLE)
                else -> throw ResponderStoreException()
            }
            sqlite.setTransactionSuccessful()
        } catch (exception: ResponderStoreException) {
            sqlite.close()
            throw exception
        } catch (_: Exception) {
            sqlite.close()
            throw ResponderStoreException()
        } finally {
            if (sqlite.inTransaction()) {
                try {
                    sqlite.endTransaction()
                } catch (_: Exception) {
                    sqlite.close()
                    throw ResponderStoreException()
                }
            }
        }
        openedDatabase = sqlite
        return sqlite
    }

    private fun loadEncoded(database: SQLiteDatabase, key: String): ByteArray? = database.query(
        TABLE_NAME,
        arrayOf(COLUMN_STATE),
        "$COLUMN_KEY = ?",
        arrayOf(key),
        null,
        null,
        null,
        "1",
    ).use { cursor: Cursor ->
        if (!cursor.moveToFirst()) null else cursor.getBlob(0)
    }

    private fun recordKey(macId: ByteArray, deviceId: ByteArray): String {
        if (macId.size != IDENTIFIER_LENGTH || deviceId.size != IDENTIFIER_LENGTH) {
            throw ResponderStoreException()
        }
        return (macId + deviceId).joinToString(separator = "") { byte ->
            "%02x".format(byte.toInt() and 0xff)
        }
    }

    private fun sameNullableBytes(left: ByteArray?, right: ByteArray?): Boolean = when {
        left == null -> right == null
        right == null -> false
        else -> left.contentEquals(right)
    }

    private companion object {
        const val DATABASE_NAME = "repose-unlock-responder-v1.sqlite3"
        const val SCHEMA_VERSION = 1
        const val IDENTIFIER_LENGTH = 16
        const val TABLE_NAME = "responder_state"
        const val COLUMN_KEY = "record_key"
        const val COLUMN_STATE = "state_blob"
        val DATABASE_NAME_PATTERN = Regex("[A-Za-z0-9._-]{1,96}")
        const val CREATE_TABLE =
            "CREATE TABLE IF NOT EXISTS responder_state (" +
                "record_key TEXT PRIMARY KEY NOT NULL, " +
                "state_blob BLOB NOT NULL" +
                ") WITHOUT ROWID"
    }
}
