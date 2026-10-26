# Retained failed full native attempt at b5132f3

Terminal exit 101. Format and lint passed; native tests stopped after 42 library cases passed and one scale-fixture assertion failed. A broad version-range replacement accidentally changed the synthetic loop from 100,000 to 110,000 blocks while its expected restored count stayed 100,001 (including the seed). Restoration produced 110,001, not the fixture expectation. No complete native PASS or release/traceability execution is inferred.

The fixture is restored to its original 100,000-iteration shape; the corrected exact case passes separately in android-cell-capture evidence. No production capacity was weakened and the failed full logs remain retained. A fresh complete native validator is still required.
