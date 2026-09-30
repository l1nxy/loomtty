# Diagnostic profiles

These macOS `sample` call graphs identify candidate hot paths in the **before**
build. They are wall-stack samples (including waits), not CPU percentages.
The profiling run uses 1,200 TUI updates at 60 Hz; each client/server sample
covers approximately ten seconds of each TUI phase. Sampling perturbs execution,
so its resource measurements are excluded from the uninstrumented comparisons.

`controller.py.txt` preserves the diagnostic launcher; `results.json` records
the instrumented run, workload settings and binary hashes. Original paths in
these files describe the machine on which the run was captured.

The profiles highlighted `viewport_row_fingerprint` on the server and row
hashing, cell-property decoding, shaping-cache lookup and glyph-cache hashing
on the client. They motivated the changes; the independent paired benchmark
determines whether those changes save CPU in practice.
