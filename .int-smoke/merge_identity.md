# Integration candidate identity — t_3864d46e

- worktree: /home/artt/Orbis-control-implementation (implementation/current-plan)
- baseline: 5ca36ab01943f4e7e1ec94e5846fe0ecb60d4c26 (pre-merge tip, clean tree)
- UI line: 860bc07 (wt/ui-symmetry, audit baseline) → 2c3e3af (F1–F4b fixes) → f45f55d (G03 fix, QA-PASSed candidate, contains 860bc07 and 2c3e3af)
- merged with: git merge --no-ff f45f55db91ad2bb9e94d11860528d227c6e1a649
- merge commit: 5f0d3da4f3da281a4bfd271ed9ef579a6ca651f0 (git SHA of the merged candidate)
- conflicts: none; both intents verified present in both overlapping files
  (cooling.slint: invalid-draft status/color lines 33/47 + Grid gutter lines 54-55;
   performance.slint: power-limit-error block line 85 + Grid/show-percent lines 36/64-65)
- runs on a common Rust tree: UI line last touched crates/ at 3457dd7 (common ancestor);
  merged Rust content == functional-line content, confirmed by renders

# Smoke instrumentation and provenance

- renderer: crates/orbis-ui/examples/ui_snapshot (Slint software renderer, offscreen; no
  display server needed — DISPLAY/WAYLAND_DISPLAY are empty in this environment)
- merged-side binary built from the merged tree: cargo build -p orbis-ui --release
  --locked --example ui_snapshot (compiled fresh post-merge, target/release/examples/ui_snapshot
  mtime 07:30:41 > merge commit 07:14)
- reference binary: .int-smoke/bin/ui_snapshot-f45f55d (bit-copy of the UI-QA build of
  f45f55d from worktree .worktrees/t_23cd8fb8, target/debug/examples/ui_snapshot)
- provenance check: baseline binary's render of about/980/light is byte-identical to
  UI-QA's own attached screenshot .qa3-shots/about-980x680-light.png
  (.int-smoke/provenance.py → identical=True)

# Smoke result

- 26 merged renders (10 sections × 980x680 dark, plus light/1200x800 variants, plus
  graphics pending / performance unsupported / performance error scenarios)
- all renders decoded, correct size, non-blank: PASS
- merged vs f45f55d normal-scenario renders: 26/26 pixel-identical
  (diff_frac=0.000000, maxdelta=0) → SC-REGRESSION hard pass
- functional scenarios on merged tree: error/pending/unsupported all differ from normal
  (diff_frac 0.0061–0.0793); performance error scenario contains red error text
  (156 red pixels) — merged functional error surface renders
- overall smoke verdict: PASS (.int-smoke/smoke_probes.log)

# Verify

- cargo build --workspace --release --locked → exit 0 (orbis-control binary rebuilt after
  merge commit; timestamp provenance 07:17:03 > merge 07:14:01)
- bash scripts/verify task → see .int-smoke/verify.log / VERITY result recorded on the card
