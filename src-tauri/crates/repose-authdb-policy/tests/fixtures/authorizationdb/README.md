# `system.login.screensaver` policy fixtures

These are inert property-list snapshots for testing pure transformations. No test
in this crate reads or writes the live authorization database.

- `stock-string.plist`: the Apple password fallback stored as a scalar `rule`.
- `stock-array.plist`: the same fallback stored as a one-item rule array.
- `third-party.plist`: `k-of-n = 1` with third-party named-rule candidates.
  Their order and unknown top-level value types must survive.
- `already-installed.plist`: the canonical Repose named-rule candidate
  immediately before the fallback.
- `missing-fallback.plist`, `duplicate-repose.plist`, `malformed.plist`,
  `wrong-class.plist`, `k-of-n-two.plist`, `dictionary-candidate.plist`,
  `duplicate-root-key.plist`, `duplicate-fallback.plist`, and
  `multi-without-k-of-n.plist`: fail-closed inputs. `misplaced-repose.plist`
  exercises surgical repair of Repose's own candidate.

The fixed v1 candidate is the named rule `ai.repose.unlock`. Task 4 deliberately
does not define or configure the plug-in mechanism behind that rule. A stock
`rule` node may be a scalar string or an array, but array candidates must all be
strings; inline dictionaries and all other candidate value types are rejected.

Unknown values and third-party candidate order are preserved at the plist value
level. `ScreenSaverPolicy::to_bytes` retains XML-versus-binary encoding, but no
API promises to retain XML whitespace/comments or binary object-table bytes.
`to_xml_bytes` is an explicit conversion helper and can reject binary-only UID
values. The later install tooling must separately define `ai.repose.unlock` as a
fixed named rule backed by the audited plug-in mechanism.

Parser resource limits are 1 MiB encoded input, 64 collection levels, 16,384
expanded events, and 256 KiB of cumulative expanded scalar/data/key bytes. The
last two limits explicitly bound binary plist DAG expansion before a `Value`
tree is allocated. Binary input is additionally required to have a canonical
contiguous object region and offset table, an acyclic object graph, and no
unreachable declared objects. Raw object and single-collection counts are
bounded before the generic plist reader can allocate their reference vectors.
