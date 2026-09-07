# Repose unlock IPC version 1

This protocol is a local, fail-closed notification and one-shot permit-consumption protocol between Apple's `authorizationhost` and the Repose unlock service. A permit-ready event is only an edge that tells the client to invoke again. Only a fresh `CONSUME_OR_WATCH` request that atomically consumes the Rust broker's non-cloneable `ConsumedPermit` and successfully advances the state machine to `Unlocking` may produce `CONSUMED`.

## Fixed frame

Every frame is exactly 84 bytes. Multi-byte integers are unsigned big-endian values. The 12-byte header was chosen because its four scalar fields fit without padding: four magic bytes, four one-byte protocol fields, and one explicit 32-bit payload length. The implementation reads and validates this header before reading the fixed 72-byte payload and never allocates from the declared length.

| Offset | Bytes | Field | Constraint |
| ---: | ---: | --- | --- |
| 0 | 4 | magic | ASCII `RPUI` |
| 4 | 1 | version | `1` |
| 5 | 1 | operation | `1` request/reply, `2` permit-ready event |
| 6 | 1 | status | shape-specific value below |
| 7 | 1 | flags | zero |
| 8 | 4 | payload length | exactly `72` |
| 12 | 32 | request nonce | CSPRNG output, never all zero |
| 44 | 4 | console UID | session selector/binding |
| 48 | 4 | audit session ID | session selector/binding |
| 52 | 8 | lock epoch | zero in requests; nonzero server binding in replies/events |
| 60 | 16 | service instance | zero in requests; nonzero in replies/events |
| 76 | 8 | watch ID | nonzero only for watching/event shapes |

The normative C constants and byte-array ABI are in `repose_unlock_ipc.h`. Rust vector tests compare every constant and offset with the encoder. Code must not cast the bytes to a packed C struct; fields are read and written byte-wise so alignment and host endianness cannot change the ABI.

## Shapes and correlation

| Name | Operation | Status | Epoch | Instance | Watch ID |
| --- | ---: | ---: | ---: | ---: | ---: |
| `CONSUME_OR_WATCH` request | 1 | 0 | zero | zero | zero |
| `CONSUMED` reply | 1 | 1 | nonzero | nonzero | zero |
| `WATCHING` reply | 1 | 2 | nonzero | nonzero | nonzero |
| `DENIED` reply | 1 | 3 | nonzero | nonzero | zero |
| `PERMIT_READY` event | 2 | 4 | nonzero | nonzero | nonzero |
| `WATCH_KEEPALIVE` event | 2 | 5 | nonzero | nonzero | nonzero |

The client can know the console UID and audit session ID, but the lock epoch is private service state. Its request therefore carries a `SessionSelector` and zero epoch. The service compares that selector with its authoritative full `SessionBinding` before consuming or registering a watch. Replies echo the request nonce and return the full binding.

A client accepts a reply only when the nonce, UID, and audit session match its request and the returned epoch and service instance are nonzero. It pins the complete `WATCHING` tuple—nonce, full binding, service instance, and watch ID—and accepts a `PERMIT_READY` or `WATCH_KEEPALIVE` event only when all of those bytes match. A keepalive carries no authority, does not consume a permit, and must only be ignored after complete correlation validation. A service restart mints a new random instance identity and requires a strictly higher lock epoch for an ongoing locked session; stale replies and queued events cannot correlate with the new service.

Unknown operations/statuses, nonzero flags, wrong versions, wrong lengths, truncated or trailing bytes, zero correlation values, and invalid shape combinations cause the connection to close without authorization. Session and peer mismatches also close instead of returning information about another session.

After writing exactly one 84-byte request, the client must call `shutdown(fd, SHUT_WR)` while keeping its read half open for the reply and optional events. Within the same initial deadline, the server must observe EOF before it touches the broker. Any byte before EOF—including a delayed trailing byte—or failure to half-close before the deadline is rejected without consuming a permit. Once that required EOF has been observed, it is not treated as a watch disconnect: a write-half close and a full close are indistinguishable through a subsequent read on macOS. After the `WATCHING` reply, the service therefore sends a fully correlated `WATCH_KEEPALIVE` at most once per second while idle. Each keepalive has its own 100 ms absolute write deadline; a closed read half or stalled peer cancels the RAII registration and frees its active-connection slot. A conforming client loops over validated keepalives without treating them as ready or authorization. This rule is also declared in the C header for the Task 6 client.

## Broker linearization

One non-poisoning outer broker mutex owns the `PermitStore`, durable replay authority, reducer state, authoritative binding, service instance, monotonic high-water, and bounded watch registry. The fixed lock order is broker, permit store, then durable replay storage.

Every time-bearing broker operation acquires that mutex before sampling monotonic time, so thread arrival order cannot create a stale pre-lock sample and a false clock rollback. `consume_or_watch` samples once while locked and then either consumes a permit and moves the resulting opaque token into the reducer, or registers one watch. There is no lookup-then-subscribe interval. The permit and reducer transition use the same time sample. A `CONSUMED` wire response is constructed only from the immutable receipt captured at that linearization point and only after the reducer confirms `Unlocking`.

Publishing installs the original core `Permit` and takes matching watches under that same mutex. Notification happens outside the broker lock. A watch is bounded, one-shot, nonce-unique, and RAII-cancelled; 128 are allowed and the 129th is denied. A ready edge still requires a second atomic invocation. Expiry, clock rollback, authority failure, session change, and restart clear or invalidate capabilities. Counter overflow either leaves state unchanged or rotates into a fresh fail-closed instance; it never prevents destructive invalidation.

## Transport and production boundary

The initial peer verification, 12-byte header read, 72-byte payload read, request EOF confirmation, broker operation, and initial response share one absolute `CLOCK_MONOTONIC`-equivalent 100 ms deadline. Partial progress and `EINTR` never renew it. A watch may remain idle, but every keepalive write and the one ready-event write use their own bounded 100 ms absolute deadlines. The initial `WATCHING` reply always precedes either event; if permit readiness races a keepalive, the non-authorizing keepalive may be written first and the ready edge follows without consuming the permit. Premature EOF, timeout, I/O error, cancellation, and thread creation failure all close without permission. Production and tests share the same RAII active-connection limiter of 128; the deadline begins when an accepted connection is dispatched, before a worker starts.

On macOS, peer verification occurs before frame parsing. It reads `LOCAL_PEERTOKEN`, requires effective UID 0 and a valid audit session/process identity, and validates the dynamic code against exactly:

```text
identifier "com.apple.authorizationhost" and anchor apple
```

There is no release `AllowAll`, environment override, verifier injection, or production socket bind. The only public production entry point requests exactly one inherited AF_UNIX/SOCK_STREAM listener from launchd under the fixed key `ConsumeSocket`, verifies that it is actually accepting, checks the expected path `/var/run/ai.repose.unlockd/consume.sock`, and requires both the real socket node and its immediate non-symlink parent directory to be root-controlled and non-writable by group/others. The socket is owner-only, and the inherited descriptor is made nonblocking and close-on-exec. Darwin versions that expose `SO_ACCEPTCONN` but return `ENOPROTOOPT` for AF_UNIX are checked through the kernel's `PROC_PIDFDSOCKETINFO` snapshot instead. Until the durable runtime is wired into that private boundary, it runs an authenticated offline processor that parses one bounded request and closes without replying. Debug tests alone may use exact-claims fakes and unique temporary socket paths.

Shutdown takes an exclusive final-send gate, rejects new dispatches, cancels watch workers, and drains bounded active connections. Consume/initial-response and ready-event writes hold the shared side of that gate only around their linearized final operation, never during an idle watch.
