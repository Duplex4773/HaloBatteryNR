# Battery insights

Open **Insights** (Alt+I) and select a device. **Refresh** reads its local retained
history; it does not query or configure hardware. The page does no background
queries while closed. Records remain limited to 30 days and summaries stream
raw observations on the existing storage worker with bounded memory.

## Battery life by polling rate

For a supported mouse, enable polling controls and use **Devices → Refresh rate**
or the tray polling menu to confirm its hardware configuration. A successful
rate-change readback also supplies evidence. Subsequent battery readings contribute to that confirmed rate's
observed drain. Requested or saved selections never establish a rate, and
historical readings without rate evidence are not assigned one retrospectively.

Rate evidence is scoped to this application session and connection. Restart,
suspend/resume, enumeration invalidation, disabling controls, a backwards clock
change or a failed configuration exchange revokes it. Use Refresh rate again to
resume learning. This feature adds no automatic hardware reads or writes.
Another application can change the rate without our knowledge: the comparison
uses the last confirmed configuration, not continuous verification. Refresh
after changing the rate elsewhere.

Only adjacent, exact, online readings with known noncharging state count.
Sleeping, stale, unknown, coarse and charging readings pause contribution.
Intervals over ten minutes, connection/session boundaries, transport changes
and rate transitions contribute neither time nor battery loss. Small percentage
rebounds do not create fake drain; only new lows within a continuous segment
increase the loss total. A rise of at least three points can indicate an
unobserved charge and is labeled inferred.

Each rate shows counted awake time, percentage points consumed, sample/drop
counts and evidence confidence. Projection evidence is listed separately: only
complete intervals between observed new-low drops within a continuous period
qualify. The first drop anchors that period. An initial or unfinished flat
percentage does not contribute to the projection; many short periods with a
single drop cannot manufacture a confident estimate. All observed usage remains
visible in the coverage totals.

A projection requires at least **30 minutes** and **three percentage points**
of matched drop-to-drop evidence. Moderate evidence requires **two hours**, **ten
points** and **three complete drop intervals**; otherwise an available projection
is tentative. These labels describe evidence amount, not a calibrated statistical
probability. Rounded estimates avoid suggesting sub-hour precision.

Full-charge awake runtime projects the measured window average drain to 100
points; it is not a measured full charge. Remaining time requires a qualifying
latest adjacent interval, and a reading no older than ten minutes at query time.
Paused, unconfirmed or stale evidence shows an explanation instead of current
remaining hours. Usage, transport and device conditions can differ across rates, so
the comparison is observational and does not prove that the rate caused the
difference. Awake time comes from battery observations, without input tracking.

## Charge summaries

The page retains at most ten recent discharge summaries, newest first, showing starting and
ending levels, counted awake time, observed consumption and average observed
drain per hour. A directly observed charge, an inferred charge and a partial
period with unknown charge start are distinguished. Observing charging does not
establish that the device reached 100%.

Sleep and rate/session changes pause accounting without inventing a new physical
charge cycle. Drops during unobserved periods are excluded from consumption,
so it can differ from the simple start-minus-end percentage. Existing history
can provide partial charge summaries without manufacturing polling-rate data.
Summaries do not measure battery capacity, health or wear.

The footer shows retained and usable discharge readings, total counted use,
confirmed-rate use, the last stored timestamp, excluded intervals and unreadable
rows. Exclusion is usually expected (charging, pauses or session changes), not
database corruption. Empty states distinguish no readings, no usable discharge
and no confirmed-rate evidence. Existing history is not assigned a rate or
rewritten to fill missing measurements.

## Recent-use prediction

The tray/dashboard estimate uses a separate recent discharge fit, while rate
comparisons summarize up to 30 days. Their results can differ. The live fit
requires exact readings explicitly known to be noncharging, at least thirty
minutes, three points and three distinct observed drops. It favors roughly the
latest ten points, widening the window for the minimum duration, and suppresses
an inconsistent fitted slope. It does not promise an exact time to empty.

Sleep, restart, unknown state and intervals longer than ten minutes add neither
usage time nor unobserved battery loss. A relative-level rebase preserves the
already learned slope across these boundaries. Charge or a rise of three points
resets it. First hardware-rate confirmation, a confirmed rate change or an
uncertain configuration write resets incompatible learning. Saved same-time
drop pairs and invalid timestamps are rejected independently on load.

Changed learning is checkpointed at most once a minute on the existing runtime
wake, as well as at orderly shutdown. Completion records preserve original poll
timestamps; availability transitions are separate events. Late results from
revoked settings/connections cannot restore old evidence. One device's rate
confirmation does not reset another device's history session.

The SQLite metadata remains compatible with existing history. There is no new
runtime dependency, worker, hardware query or timer. A read-only developer audit
is available via `cargo run -p hb-storage --example audit_insights -- [database]`.
Its output uses ordinal device labels and omits names, identities and paths.
