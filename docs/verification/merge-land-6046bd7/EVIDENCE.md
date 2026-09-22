# LAND stage evidence — candidate 6046bd7996fbad8f5d768eeaaf76d9a501251226

Generated: 2026-09-22T10:16:19Z (UTC)

## Merge identity

    git rev-list --parents -n1 HEAD
    6046bd7996fbad8f5d768eeaaf76d9a501251226 f62a5ba805353ea9a254bf32ab4baa72ad0c1daf e1a409c402646a2fa84e24fa06d2186cee18954f

True two-parent merge commit:
- parent 1 = `f62a5ba` (implementation/current-plan, ours)
- parent 2 = `e1a409c` (origin/agent/finish-v01 tip, theirs)

Single candidate commit in this checkout. The tracked tree is exactly this commit;
evidence files under this directory are untracked so the verified SHA is unchanged.

## Exit gate — commands and results (run on 6046bd7)

    $ cargo fmt --all -- --check
    exit 0  (FMT_EXIT=0)

    $ cargo check --workspace --locked --keep-going
    exit 0  (Finished dev profile; 0 errors)

    $ cargo test --workspace --locked
    exit 0  — 1558 passed, 0 failed, 0 regressions
    (full log: gate-full.txt in this directory, 129080 bytes)

## tests-green: test-attribute counts per line

    ours   f62a5ba = 1552
    theirs e1a409c = 1387
    merged  6046bd7 = 1603

Merged count is strictly greater than either line alone.

## automation-kept

    $ git ls-tree -r --name-only HEAD | grep -c automation_
    21

Ours had 21 automation_* files; theirs had 0 (`0c8e37b` deleted them). All 21 are
retained in the merged tree and declared as hardware-inert scaffolding.

## merge-lands: feature inventory (both lines present)

Ours-side:
- power limits pipeline (`PowerLimitsTuple` / `power_limit_field`): 6 files
- product GPU wire contract (`ProductGpuMutationResult`): 6 files
- fan production path (`FanServiceRuntime` / `set_fan_curve`): 14 files
- LACT: 5 files

Theirs-side:
- `ValueSlider` on rgb-red / rgb-green / rgb-blue: 7 references in backlight.slint
- secondary RGB (`aura-secondary-red/green/blue`): 10 references
- 12 distinct Aura effect modes (0–8, 10, 11, 12): 13 request-effect call sites
- clamshell: 9 files
- CPU package limits (`cpu_package_power_limits`): 8 files
- POST sound (`BootSoundState`): 7 files

## Post-merge fixes folded into the candidate

1. `crates/orbis-ui/src/worker_runtime.rs` — `WorkerEvent::FanCurve` retyped to
   `SetFanCurveError` (both lines' alias = `CommandError`); test-profile compile was
   broken by the stale `ProviderError` variant.
2. `crates/orbis-session-client/src/lib.rs` — `zbus_error_to_provider` `MethodError`
   arm no longer short-circuits; zbus marshals a wrapped handler error under the
   generic name `org.freedesktop.zbus.Error` with the real name in the detail, so the
   detail-based `AccessDenied` fallback must still run. Restores
   `ProviderError::PermissionDenied` for the delegated-client P2P test.
3. `crates/orbis-ui/src/main_tests.rs` — adapted ours' stale product-GPU fixtures to the
   corrected wire→card mapping and replaced the nav test that pinned internals of a
   component their line replaced.

## Wire→card mapping decision (evidenced, not assumed)

Both lines encode the product GPU wire identically in `orbis-hardwared`:
Hybrid=0, Integrated=1, Ultimate=2. Both lines' UI cards are Eco=0 (iGPU-only),
Standard=1 (Hybrid), Ultimate=2. Therefore wire Hybrid (0) selects the Standard card (1)
and wire Integrated (1) selects the Eco card (0). Ours' identity mapping (0->0) was a
genuine inversion contradicted by its own "Только iGPU" Eco label; the merge keeps
theirs' corrected swap in production code and only ours' test fixtures were adapted.

## Scope boundaries honoured

- No push to origin; `main` untouched; no release tag created.
- Requirements and acceptance criteria unchanged.
- Merge is a true two-parent merge commit; no `-X ours` / `-X theirs` blanket mode.
