package ai.repose.mobile.unlock.responder

import java.io.File
import java.io.RandomAccessFile
import java.nio.channels.FileLock
import java.nio.channels.OverlappingFileLockException
import java.nio.file.Files

/** Cross-process lease for one member of a fixed, crash-recoverable resource pool. */
internal class EphemeralSlotLease private constructor(
    val slot: Int,
    private val randomAccessFile: RandomAccessFile,
    private val fileLock: FileLock,
) : AutoCloseable {
    private var open = true

    @Synchronized
    override fun close() {
        if (!open) return
        open = false
        var failed = false
        try {
            fileLock.release()
        } catch (_: Exception) {
            failed = true
        }
        try {
            randomAccessFile.close()
        } catch (_: Exception) {
            failed = true
        }
        if (failed) throw ResponderUnavailableException()
    }

    companion object {
        fun acquire(
            noBackupDirectory: File,
            slotCount: Int,
            lockFilePrefix: String,
        ): EphemeralSlotLease {
            validatePool(noBackupDirectory, slotCount, lockFilePrefix)
            repeat(slotCount) { slot ->
                tryAcquireSlot(noBackupDirectory, slot, lockFilePrefix)?.let { return it }
            }
            throw ResponderUnavailableException()
        }

        fun acquireAndSweep(
            noBackupDirectory: File,
            slotCount: Int,
            lockFilePrefix: String,
            sweepUnlockedSlot: (Int) -> Unit,
        ): EphemeralSlotLease {
            validatePool(noBackupDirectory, slotCount, lockFilePrefix)
            var retained: EphemeralSlotLease? = null
            try {
                repeat(slotCount) { slot ->
                    val lease = tryAcquireSlot(noBackupDirectory, slot, lockFilePrefix)
                        ?: return@repeat
                    var keep = false
                    try {
                        sweepUnlockedSlot(slot)
                        if (retained == null) {
                            retained = lease
                            keep = true
                        }
                    } finally {
                        if (!keep) lease.close()
                    }
                }
                return retained ?: throw ResponderUnavailableException()
            } catch (_: Exception) {
                try {
                    retained?.close()
                } catch (_: Exception) {
                    // The OS still releases the lease on descriptor or process termination.
                }
                throw ResponderUnavailableException()
            }
        }

        private fun validatePool(
            noBackupDirectory: File,
            slotCount: Int,
            lockFilePrefix: String,
        ) {
            if (slotCount <= 0 || lockFilePrefix.isEmpty() ||
                (!noBackupDirectory.exists() && !noBackupDirectory.mkdirs()) ||
                !noBackupDirectory.isDirectory
            ) {
                throw ResponderUnavailableException()
            }
        }

        private fun tryAcquireSlot(
            noBackupDirectory: File,
            slot: Int,
            lockFilePrefix: String,
        ): EphemeralSlotLease? {
            val lockFile = File(noBackupDirectory, "$lockFilePrefix$slot")
            if (Files.isSymbolicLink(lockFile.toPath())) {
                throw ResponderUnavailableException()
            }
            val handle = try {
                RandomAccessFile(lockFile, "rw")
            } catch (_: Exception) {
                throw ResponderUnavailableException()
            }
            val lock = try {
                handle.channel.tryLock()
            } catch (_: OverlappingFileLockException) {
                null
            } catch (_: Exception) {
                try {
                    handle.close()
                } catch (_: Exception) {
                    // Report the original unavailability without leaking a platform exception.
                }
                throw ResponderUnavailableException()
            }
            if (lock != null) return EphemeralSlotLease(slot, handle, lock)
            try {
                handle.close()
            } catch (_: Exception) {
                throw ResponderUnavailableException()
            }
            return null
        }
    }
}
