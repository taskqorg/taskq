# Mac legacy import health

Command: `sh scripts/telemetry.sh health` (default 150-second window), run on the Mac host. [Health output](health.txt) ends with verdict PASS.

The service reports the same version as the CLI that produced the output: 4.6.77-261104-6. Local paths are removed from the release line; every other line is verbatim.

The output reads `contains 1e5ff561: unknown` because a dev build has no release tag to compare. The dev install marker names source commit 026db252, and `git merge-base --is-ancestor 1e5ff561 026db252` exits 0, so this build contains the fix. The finite window proves observed import health, not indefinite future health.
