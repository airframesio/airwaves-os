# Airwaves OS Roadmap

Forward-looking work. Current behavior is documented in the other files under
`docs/` and in the top-level `CLAUDE.md`.

## System updates

### Manifest distribution
- **Now:** release manifest is plain JSON in this repo, served via
  `raw.githubusercontent.com` (`releases/<channel>.json`). Simple, free, and
  versioned in git. `stable.json` tracks `main`.
- **Next:** publish manifests as **GitHub Release assets** and pin component
  `url`s (compose/catalog) to the release **tag** rather than `main`, so a
  given manifest revision is immutable.
- **Later:** a dedicated update service / CDN at **`updates.airwavesos.com`**
  for channels (stable/beta), staged rollout, signing, and basic telemetry
  (opt-in).

### Atomic, rollback-safe OS updates — A/B partitions
The current updater does **in-place** apt upgrades (including major Debian
release upgrades). This is powerful but not atomic: a failed major upgrade can
leave the rootfs in a partial state, and image rollback is best-effort because
compose pins `:latest`.

The target design is **A/B (dual-root) partitioning**:
- Two root partitions (A/B) plus a shared data partition for `/etc/airwaves`,
  Docker volumes, and config.
- Updates are written to the **inactive** slot, then the bootloader is switched
  to it; the previous slot remains intact for instant rollback.
- A health-gate after first boot auto-rolls-back if the new slot fails to come
  up healthy.
- Container image pinning by digest so image rollback is exact.

Tracked in GitHub issue [#35](https://github.com/airframesio/airwaves-os/issues/35):
**A/B partition scheme for atomic, rollback-safe OS updates**.

## Other
- Beta channel + opt-in auto-apply for `recommended`/`required` updates.
- Signed manifests + image digest pinning end-to-end.
