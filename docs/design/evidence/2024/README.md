# #2024 evidence

From `shots.yml` run [41178291809](https://github.com/taskqorg/taskq/actions/runs/41178291809), dispatched on
branch `taskq-2024` with `ref=v4.6.52` (macos-latest, arm64, display scale 1). A branch run uploads artifacts only.

- The workflow downloaded the `taskq-4.6.52-macos-aarch64` release asset; `update.sh v4.6.52 --bin … --publish`
  verified it against the release `SHA256SUMS`, `gh attestation verify` and `--version`, then captured at
  `73df356b19e841e3900c06878206bfdd73b7c41e`.
- Log: `update.sh: staged in 22 s from capture start`; `docs/shots updated to 4.6.52 (release v4.6.52, 73df356b…)
  in 25 s from capture start`, preview included.
- `manifest-4.6.52.json`: the manifest it wrote (version, source commit, binary origin and hash, environment, UI
  source blobs, capture input hashes, image hashes, reuse key).
- `readme-1280.png`, `readme-390.png`, `page-checks.json`: `scripts/readme-preview.cjs` output with the staged
  images; every image loaded and no horizontal overflow at either width.

The images themselves stay out of this branch (#1841); the release dispatch on `main` delivers them as a draft PR.
