# #802 draft delivery evidence

Planning previews only. [Scene, timing, style and Dmitriy approval](approval.md#recorded-owner-decision)
is recorded without a listening review.

## Retained Windows delivery

The previous attempt added new filenames to the existing
[unpublished A draft](https://github.com/taskqorg/taskq/releases/tag/untagged-9df4398faaa4cfb1b3cd)
without replacing historical assets. Fresh release readback confirms `isDraft=true`.
No public release, final synthesis, final assembly or production activation.

| Download (requires repository access) | Bytes | SHA-256 |
|---|---:|---|
| [Silent animatic](https://api.github.com/repos/taskqorg/taskq/releases/assets/635400959) | 683183 | `3176ee37ba3db6c4840e4f4c934c97b4508308e38b07c5931e061762952ee15a` |
| [Mobile sheet](https://api.github.com/repos/taskqorg/taskq/releases/assets/635400973) | 344919 | `57357051d60b29fac317ee67454d382d774dc1d00f570f6ac62da3cf701a5eab` |
| [Source ZIP](https://api.github.com/repos/taskqorg/taskq/releases/assets/635400958) | 8469 | `00d6dfb41a7f372ffced7f1d0f08955df63d32e52fbb4e3bff79a457abb26bd3` |
| [11 master/mobile style pairs](https://api.github.com/repos/taskqorg/taskq/releases/assets/635400951) | 1004404 | `57c95ce7d76dfecc1e37cdfdd3ccefacc3d18dae3a452653205f21bab1f2903d` |
| [SHA256SUMS](https://api.github.com/repos/taskqorg/taskq/releases/assets/635400952) | 367 | Four media/archive hashes above |

Downloaded all five assets into a separate directory after upload; all four
media/archive SHA-256 values match the uploaded manifest and local originals.
Source ZIP has exactly `render.py`, `scenes.json` and `approval.md` at their
repository-relative paths. It is reproducible when extracted into a checkout;
linked historical references and this delivery receipt remain in the repository.
Style ZIP has 22 PNGs: 1080×1080 and 360×360 for each scene; sheet is 1080×1440.
No voice samples, private source, weights, fonts, environment or credentials
were copied into either new archive. Retained voice links/hashes are in
[approval.md](approval.md#retained-voice-audition-and-provenance).

```sh
gh release download taskq-preview-585 --repo taskqorg/taskq --pattern 'planning-*-802.*' --pattern 'planning-mobile-sheet.png' --dir .taskq/802-download
```

Windows media qualification: FFmpeg/ffprobe 9.0.2, libx264 CPU encoder;
fresh probe reports H.264, yuv420p, 1080×1080, 10 fps, 580 frames, exactly
58.000000 seconds and no audio stream. Both local and freshly downloaded MP4
fully decode without errors. Decoded 26 s mobile frame independently confirms
the question card. Source assertions check every frame's text bounds, timeline
boundaries, increasing holds of at least 4 seconds and invalid-time rejection.
Mobile sheet inspected at 360 pixels per cell; actual phone playback and owner
listening/creative preferences are unverified. No paid provider calls;
provider charge none, worker/host cost unknown.

Scene/narration data, renderer and approval notes are the candidate source;
the generated binaries stay out of git. Final motion, subtitle synchronization,
natural voice pacing, safe site reveal asset, real board capture and phone QA
remain gated production work within #802 after the scope consolidation. Follow
the #802 taskq approval card,
not historical closed tasks, before progressing.

## Mac continuation readback

Checked against `b0828fb5ca4c6930faec1c3882c4fd8c2a95bbcf` and TaskQ's
[owner instruction](https://github.com/taskqorg/taskq/issues/802#issuecomment-6445677156).
The old untagged release URL resolves to release `408154120`, current tag
`taskq-preview-585`; fresh API readback reports `draft=true`. No asset was
uploaded, replaced or published in this continuation.

- **Pass:** Fresh downloads of all five planning assets and four retained voice
  assets match the SHA-256 values above and in [approval.md](approval.md).
  The checksum manifest itself is 367 bytes, SHA-256
  `4ce6de2124d38651644bd9f6b42f8c38df0d58861d5662ac4f37ccf539e9bd59`.
- **Pass:** Standard-library ZIP CRC checks; 22 style PNG headers retain the
  1080/360 square dimensions. Archived `render.py` and `scenes.json` match this
  checkout byte for byte. The archived approval note is the original historical
  note; this checkout's linked decision supersedes its pending-approval wording.
- **Pass:** Standard-library WAV reads confirm both full PCM payloads, mono,
  16-bit, 48 kHz, Dmitriy 13.375 s and Eduard 11.900 s. This is no listening test.
- **Pass:** Retained `LICENSE_CIS` matches pinned upstream revision
  `d9355348e2781dc8fa25a135d1602c530afae24c` byte for byte; its `models.yml` names
  the retained CIS Base nostress URL. No model weights were downloaded or run.
- **Pass:** Existing Python 3.12.1 / Pillow 12.0.0 and installed Arial regular/bold run
  `render.py --check`: 11 boundaries, 58 seconds, all 580 frame text bounds.
  Default Python lacks Pillow; no package was installed. The retained mobile
  contact sheet was inspected; no new preview was generated.
- **Not tested:** Mac FFmpeg/ffprobe encode/decode and playback. Homebrew ffmpeg 8.1.1
  provides both executables, but the existing tool answer authorizes Windows only. Request
  exact Mac tool use through #802 before running them; no installation is needed.
- **Not tested:** Human phone playback, voice listening, final subtitles, natural
  pacing, final site asset and seamless-loop motion. These are not established
  by the planning approval or file hashes.

R12 preserves the media/runtime unknowns. R13 records the approved planning target
in the contract before updating the package; §7 keeps final production and old
sessions inactive. Before this change the documents still said all choices were
pending, linked the old repository, and treated #805 as separate work. The
continuation corrects those records without changing scene data or media.

### Gate failure and retained protection

The first canonical gate failed: 541 Rust checks passed, 18 failed, 32 were
ignored. The new contract pointer exceeded the existing 60 KiB limit by 201
bytes; the unchanged golden-trace/size check caught it. The pointer and opening
prose were shortened without changing rule meanings; the limit remains 60 KiB.
The other 17 failures inherited worker identity and refused host-owner fixture
operations. A gate run without `TASKQ_TASK`, `TASKQ_ATTEMPT` and `TASKQ_ROLE`
confirmed 558 passed with only the contract-size failure remaining. This
isolation applies only to test subprocesses, never board commands or the worker.
No test, assertion, host grant or runtime policy was changed. Final gate outcome
is reported on the candidate PR; failures are not passing evidence.
