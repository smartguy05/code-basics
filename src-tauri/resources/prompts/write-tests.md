---
title: Write Tests
id: write-tests
---
Write tests for the change I just described, tests-first.

1. Add the failing test **before** the implementation, and run it — confirm it fails for the right reason (it names the symptom, not the fix).
2. Cover the happy path plus the edge cases that actually matter: empty input, boundaries, error/abstain paths.
3. Keep each test focused and named for the behaviour it pins.
4. Run the suite again after implementing and paste the real output — do not summarise a pass you did not see.

Do not weaken or delete an assertion to get green: if a test is wrong, say so explicitly; otherwise the code is.
