# Battery insights

Open **Insights** (Alt+I) and select a device. **Refresh** reads its local retained
history; it does not query or configure hardware. The page does no background
queries while closed. Records remain limited to 30 days and summaries stream
raw observations on the existing storage worker with bounded memory.

## Battery life by polling rate

For a supported mouse, enable polling controls and use **Devices → Refresh rate**
to confirm its hardware configuration. A successful Apply readback also supplies
evidence. Subsequent battery readings can contribute to that confirmed rate's
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
counts and evidence confidence. A projection requires at least **30 minutes**
and **three percentage points** of counted discharge. Moderate evidence requires
at least **two hours**, **ten points** and **three observed drops**; otherwise
an available projection has low confidence. These labels describe evidence
amount, not a calibrated statistical probability.

Full-charge awake runtime projects the observed average drain to 100 points; it
is not a measured full charge. Remaining time refers to the last qualifying
reading. Usage, transport and device conditions can differ across rates, so
the comparison is observational and does not prove that the rate caused the
difference. Awake time comes from battery observations, without input tracking.

## Charge summaries

The page retains at most ten recent discharge summaries, showing starting and
ending levels, counted awake time, observed consumption and average observed
drain per hour. A directly observed charge, an inferred charge and a partial
period with unknown charge start are distinguished. Observing charging does not
establish that the device reached 100%.

Sleep and rate/session changes pause accounting without inventing a new physical
charge cycle. Drops during unobserved periods are excluded from consumption,
so it can differ from the simple start-minus-end percentage. Existing history
can provide partial charge summaries without manufacturing polling-rate data.
Summaries do not measure battery capacity, health or wear.

The SQLite upgrade adds optional rate/session metadata alongside unchanged
reading payloads. Original charts, settings and learned remaining-use behavior
are preserved. No new runtime dependency, worker or timer is required.
