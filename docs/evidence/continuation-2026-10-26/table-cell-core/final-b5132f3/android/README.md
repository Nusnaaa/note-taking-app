# Complete Android verification of native table-cell Core

All nine checks passed at clean b5132f3, with terminal exit zero and finished_utc. Android: 373 tests, no failures/errors/skips; desktop JNI: 52 groups before and after fresh R8 optimisation. Exact 383-file source map, logs and test XML retained. This precedes the later Android cell journal/controller changes and does not verify them.

The native b5132f3 attempt failed a scale-fixture count assertion and is retained separately under failed-full-b5132f3; this Android success does not make the paired native run pass or close backend acceptance.
