# Battery history

History defaults to **Time used**. Choose the last 2, 8 or 24 hours of recorded
use. If less use is available, the graph fits that available time. The axis uses
minutes and hours rather than calendar days.

Use time is an estimate from device battery observations, not cursor movement or
button activity. An interval counts only when both adjacent observations are
online, have valid battery percentages and are not charging. Sleeping, stale,
unknown and charging observations pause the clock. Each observed interval is
capped at ten minutes, matching the existing awake-use safeguard for long
unobserved gaps. The app does not add input hooks or wake the mouse to collect
history. Previously recorded data can be displayed without a schema migration.

Select **Calendar time** for the last 24 hours, 7 days or 30 days. The graph holds
the last known battery percentage until another valid awake reading arrives,
including across sleep and temporary communication failures. Battery changes
appear at their observation time as steps. Dots mark available awake readings;
the held line is not a newly measured percentage. A retained earlier reading
can seed the start of the selected interval. Without a known percentage, the
graph stays empty until the first available reading.

Both views use the same 30-day retention. Use-time totals are computed from raw
retained observations before downsampling, so display width does not change the
calculated duration. Queries run on the existing storage worker with bounded
display memory; dashboard graphics are released on close. Real timestamps and
connection states remain in SQLite. Display coordinates and carried levels are
not written back as synthetic observations, and discharge estimates are not
changed by the graph.
