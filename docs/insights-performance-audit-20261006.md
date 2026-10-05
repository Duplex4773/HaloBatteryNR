# Insights and allocation follow-up — 6 October 2026

This pass starts from `21c90c1`. It reviews further CPU/RAM opportunities and
Insights correctness, storage, UI messaging and query scheduling. No dependency,
thread, device packet, automatic rate change or background history query was added.

## Implemented savings

| Path | Before | After |
|---|---|---|
| Unchanged five-second freshness tick, status export off | Rebuild the complete device snapshot, strings and estimates; process the inventory again in the UI. | Send only a timestamp. The UI retains its snapshot and refreshes visible freshness text and existing tray/theme checks. Real changes still publish immediately. |
| History batch serialization, 512 synthetic rows | 2,048 Rust allocation calls, 468,992 allocated bytes. | 2 calls, 532 allocated bytes, using reusable JSON and session buffers. |
| Successful recovery after a 2,048-reading batch | Retain 475,136 bytes of empty vector capacity. | Retain 14,848 bytes; release oversized capacity after successful flush. Failed flushes keep their records intact. |
| Repeated Insights Refresh clicks while loading | Queue another full retained-history scan for every click. | Share the pending request for that device; a rejected request clears pending state so Refresh can retry. |

Allocation measurements include the real SQLite flush path using the same
512-row fixture before and after the edit. They count Rust allocation traffic,
not SQLite's internal allocator or whole-process private memory. Freed vector
capacity returns to the allocator; this does not promise an equal immediate drop
in Task Manager. Normal small batches retain their buffer to avoid churn.

The five-second freshness cadence is unchanged. Opt-in status export retains
its existing full snapshot and file-update behavior. A failed UI snapshot send
is retried at the normal cadence; a lightweight tick cannot replace missing
state or cause a busy retry loop. No provider scheduling interval was lengthened.

## Insights findings

- Repeated small battery rebounds after sleep or across session/rate/transport
  boundaries could count the same percentage drop again. A failing regression
  reproduced **14 points counted for a two-point decrease**. Consumption now
  respects the cycle's prior low as well as the current continuous segment's low.
  Unobserved gap drain remains excluded and a rise of three points still starts
  an inferred charging cycle. Projections retain the existing evidence thresholds.
- Malformed rate metadata could become an apparently valid unknown-rate interval.
  Invalid numeric values, strings and blobs now break continuity, matching the
  existing policy for malformed session metadata. Valid legacy NULLs still work.
- The UI described an estimate as based on all recorded use, including time
  excluded from its projection. It now shows projection duration separately from
  total recorded use, and labels remaining time as applying to the last saved reading.
- A sub-hour projection could display “about 0 h” in the rate list while its
  detail used different wording. Both now say “less than 1 hour”.
- Devices without supported polling controls were instructed to enable them.
  They now receive an explanation and retain their battery-session summaries.
- Failed refreshes retained old results; queue rejection could leave loading
  pending. Failures now clear obsolete results or pending request state and offer
  a retry. Closing or changing device still rejects stale replies.

Read-only inspection of a consistent copy of the local history found no integrity
errors, malformed readings, identity/level mismatches, future records, orphaned
metadata or invalid polling metadata. The new rebound fix did not change the
observed summaries in that copy. Raw records, names, identifiers, timestamps and
detailed local audit output stay under ignored `validation-local`; no user history
was rewritten. Valid records and few observed drops do not establish a reliable
battery-life prediction or a measurement of battery health.

## Validation

Formatting, strict workspace Clippy, **588 tests**, the 459-ID upstream coverage
gate, release build, production API/import restrictions and privacy scans pass.
Two optional SQL timing tests remain excluded from the ordinary suite. Native
tests cover both themes, forty dashboard cycles, freshness and queue backpressure.
Updated Insights screenshots use invented data only and were visually inspected.

The unsigned portable executable is **3,053,056 bytes (2.91 MiB)**, SHA256
`19862B19E25F3C59B90E6D2871A4B7341AF2CDCB474F059DEAF77A84EBE56A50`.
Package: `0.1.0-insights-audit-20261006`.

The optimized native fixture ran two three-minute windows with one simulated
mouse, no charging animation and the dashboard closed:

| Metric | Before 40 dashboard cycles | After 40 dashboard cycles |
|---|---:|---:|
| Duration | 180.04 s | 180.02 s |
| Average private memory | 7.18 MiB | 7.78 MiB |
| Peak private memory | 22.89 MiB | 7.89 MiB |
| CPU, one logical core | 0.148% | 0.165% |
| Closed-dashboard GDI / USER handles | 108 / 52 | 108 / 52 |

These are lifecycle phases of the same optimized test fixture, not before/after
code measurements. Synthetic screenshot generation precedes the windows, so
they should not be compared directly with earlier captures-disabled runs. This
workload remains within the 30 MiB and 0.5% unanimated targets; it does not establish
a whole-process CPU or RAM reduction, a hardware benchmark, or animated performance.
The code changes reduce the measured allocation traffic and retained recovery
capacity, without forcing working-set trimming. The running normal app was left
untouched.

The concurrent-save stress test encountered an extra temporary filename the app
does not generate. Its cleanup assertion now checks the app's exact temporary
filename format, leaving foreign files alone; final file completeness and all
writer results remain checked. No antivirus settings were changed.

Reproduce allocation and recovery checks with:

```powershell
cargo test -p hb-storage --test performance_regressions batched_history_flush --locked -- --nocapture
cargo test -p hb-storage successful_recovery_releases_oversized_history_batch_capacity --locked -- --nocapture
cargo test -p hb-core --test insights --locked
cargo test --workspace --locked
```

## Further opportunities and tradeoffs

- Status-export users could benefit from shared snapshot ownership; the default
  export-off path already avoids that copy. Changing export frequency would affect
  integrations and is not part of this pass.
- Absent-provider backoff could reduce recovery discovery work, but needs matched
  hardware testing to avoid slower connection detection after missed Windows events.
- Sharing equivalent charging frames could help multiple identical icons. It
  adds cache ownership complexity and offers little benefit for a single mouse.
- Smaller SQLite caches or shorter worker stacks risk more disk I/O or stack
  exhaustion. Existing bounded caches and stacks were retained.

These remain candidates, not measured savings. Existing resource targets still
apply; reducing functionality or forcing working-set trimming is not counted as
an optimization.
