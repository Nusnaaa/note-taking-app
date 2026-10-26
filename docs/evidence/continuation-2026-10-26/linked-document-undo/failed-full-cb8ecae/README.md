# Failed full native validation of cb8ecae

Session63488 terminated exit101. Format/lint passed, tests failed at pressure_response::legacy_layout_stream_reads_schema15_without_inventing_pressure_settings. The constructed old stream still expected table32 at the tail, but schema22 appends empty edit_links table33. Its assumption failed before legacy restore was attempted. Exact raw output/source hashes are retained; no finished_utc or successful completion is invented. A later focused fixture correction does not turn this run into a pass.
