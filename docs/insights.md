# Battery insights

Open **Insights** (Alt+I) and select a device. **Refresh** reads its local retained
history; it does not query or configure hardware. The page does no background
queries while closed. Records remain limited to 30 days and summaries stream
raw observations on the existing storage worker with bounded memory.
Repeated Refresh clicks share an already-pending request instead of queuing
duplicate history scans. The footer shows the last saved reading's date and time;
remaining-use estimates refer to that reading, rather than a continuously updated
countdown. A failed refresh clears the old result and offers a retry.

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
rebounds do not create fake drain, including across sleep, transport, rate and
session boundaries. A counted drop must be below both the current uninterrupted
segment's low and the physical cycle's low. Drops during unobserved gaps remain
excluded. A rise of at least three points can indicate an
unobserved charge and is labeled inferred.

The page distinguishes total counted use from the shorter duration supporting an
estimate. Technical sample/drop counts and confidence remain available in support
reports. Only
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

## Dynamic polling rates

Automatic boost and restoration use the same hardware-confirmed rate evidence as
manual changes. Their battery observations appear under the actual rate (for
example, 1000 and 2000 Hz), without splitting one discharge into separate physical
battery sessions. Even a boost and restore between two battery samples breaks
continuity: the interval is excluded rather than attributed to the normal rate.

The summary below the sessions shows the configured boost rate and recorded time
at that rate as a share of all confirmed-rate use. This includes manual use at the
same rate; it does not measure gaming time or prove which action selected a rate.
Use Refresh to include the newest retained samples. Existing history needs no
migration and rates are never inferred from requested settings.

Once every represented rate has enough independent evidence, Insights estimates
full-charge life for the recorded mix. It weights drain by counted time:
`mixed hours = total seconds / sum(seconds at rate / full-charge hours at rate)`.
For example, two hours at a rate with a 100-hour projection and one hour at a rate
with a 50-hour projection imply 75 hours for that mix, not an arithmetic average.
A newly configured boost with no recorded use keeps this estimate in learning.
Unknown-rate periods, pauses and switching intervals are excluded. Short sessions
with insufficient drop-to-drop evidence cannot manufacture an estimate. This is
an observational historical mix, not a prediction of future gaming habits or a
new live tray estimate. It adds no timers, hardware queries or database tables.

Local validation on 9 October 2026 passed 607 workspace tests, strict Clippy and
formatting. Regressions cover 1000 → 2000 → 1000 Hz accounting, switches entirely
between battery samples, insufficient/malformed projection evidence, weighted
drain math and native summary visibility in both themes. No new hardware or
performance measurement was performed for this on-demand summary calculation.

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

The footer shows total counted use and the last stored timestamp, and flags unreadable
rows. Detailed retained/usable reading counts and excluded intervals are included
in support reports. Exclusion is usually expected (charging, pauses or session changes), not
database corruption. Empty states distinguish no readings, no usable discharge
and no confirmed-rate evidence. Existing history is not assigned a rate or
rewritten to fill missing measurements.
Devices without a supported polling route receive an appropriate explanation;
they are not directed to unavailable rate controls. Invalid rate/session metadata
breaks continuity rather than being treated as ordinary legacy history.

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
