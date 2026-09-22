# UI-A · Aura/backlight live verification — task t_3798d9da

Surface: `ui/audited/sections/backlight.slint` (+ `ui/components/value-slider.slint`).
Acceptance: `SC-VISUAL-STATES`. Candidate: `6046bd7996fbad8f5d768eeaaf76d9a501251226`
(branch `implementation/current-plan`; chain label `orbis-v01-consolidated`).

## Outcome

**No code change was required.** The live application already satisfies the contract on
this base. Per the card ("If the surface already satisfies the contract in the live
application and nothing needs changing, that is a legitimate finding: record the
measurement as evidence and report the current HEAD"), no commit was made and the
candidate is the current HEAD `6046bd7`.

`git diff --stat -- ui/ crates/orbis-ui/src/` is empty → the backlight/Aura surface is
byte-identical to HEAD.

## Live target identity

| Fact | Value |
|---|---|
| App | `./target/release/orbis-control` (release) |
| PID | 1868447 (uptime ~20 min at time of measurement) |
| Binary mtime | 2026-09-22 14:17:05 +0300 |
| Binary sha256 | `ea99bd67ffd25cdb7b69c74221d8aa394aacce6f7fbb517a18038c3513dc7203` |
| HEAD committed | 2026-09-22T13:15:56 +03:00 |
| Source newer than binary | none (`find crates/orbis-ui/src ui -newer <binary>` → empty) |
| Session | DISPLAY=:0, KWin Wayland, window 1240x820 at (340,107) |
| Input | uinput pointer helper (`target/aura-live/uinput_mouse.py`) + `kdotool` read-back |

Binary is strictly newer than the HEAD commit and no source file is newer than the
binary ⇒ the running process executes HEAD `6046bd7` source. The run is a real
application on a real display, not an offscreen render and not demo data: the RGB/effect
state shown by the UI is read back from asusd over D-Bus.

## Hardware boundary used

```
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/aura/tuf xyz.ljones.Aura LedModeData
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/aura/tuf xyz.ljones.Aura LedMode
busctl --system get-property xyz.ljones.Asusd /xyz/ljones/aura/tuf xyz.ljones.Aura Brightness
```

Live capability evidence for this machine (read, not assumed):

```
SupportedBasicModes = au 0        (Static only)
SupportedBasicZones = au 0
SupportedBrightness = au 4 0 1 2 3
Brightness          = u 3
DeviceType          = u 2
```

## aura-sliders — PASS

Every colour control group was driven by real pointer input and the applied value was
confirmed by reading asusd back (`LedModeData` colour1 = `(r,g,b)`):

| Step | Control | asusd read-back after the action |
|---|---|---|
| baseline | — | `0 0 0 246 255 0 0 0 "Med" "Right"` |
| click preset «Красный» | `ColorPresetButton` | colour1 → `255 65 77` |
| drag Красный | `ValueSlider` r | colour1 → `130 65 77` |
| drag Зелёный | `ValueSlider` g | colour1 → `130 129 77` |
| drag Синий | `ValueSlider` b | colour1 → `130 129 139` |
| drag Вторичный-зелёный | `ValueSlider` secondary g | colour2 → `0 130 0` |
| drag Вторичный-синий | `ValueSlider` secondary b | colour2 → `0 130 135` |
| drag Вторичный-красный | `ValueSlider` secondary r | colour2 → `133 130 135` |
| mid-drag capture | drag red 900→960 | colour1 r → `229` (preview while gesture active) |

All four slider groups (red, green, blue, secondary colour) moved, the value was applied
to hardware, and the change is observable in the running application. The mid-drag frame
(`live-12-red-drag-mid.png`) shows the local preview moving during the gesture, and the
settled frame shows the committed value — i.e. the "preview locally, `changed()` once on
release" contract of `value-slider.slint` behaves as specified.

## aura-effects — PASS

The 12-effect set is present and observable in the live card. Rendered state, classified
by sampling each button's background/border pixels against the dark-theme palette
(`Palette.surface-disabled` #181D22 vs `Palette.surface-raised` #1E242B vs
`Palette.accent-soft` #172A45) — full output in `measure-effects.txt`:

```
Static        SELECTED
Breathe Rainbow Star "Rainbow Wave" Rain Highlight Laser Ripple Pulse Comet Flash  -> disabled
Slow Normal   enabled
Fast          SELECTED
totals: {'SELECTED': 2, 'disabled': 11, 'enabled': 2}
```

Effect selection and speed selection both reach the hardware:

* speed click «Fast» → asusd speed name `"Med"` → `"High"` (read-back confirmed);
* effect click «Static» is the selected mode and `LedMode` reads back `0`.

Clicking a disabled effect (`Breathe`, `live-10-breathe-disabled.png`) produced **no**
hardware change — the button is genuinely inert, not a decorative control.

### Why 11 of 12 are disabled — honest capability, not a UI defect

asusd on this ASUS TUF reports `SupportedBasicModes = au 0`, i.e. Static only. The UI
derives per-mode support directly from that runtime evidence
(`extra_backend.rs:860-871` ← `supported_modes` ← `SupportedBasicModes`) and
`mode-supported()` in `backlight.slint:125-139` disables the button. This matches the
project invariants "capability support is based on runtime evidence, not laptop
model-name guesses" and "unsupported or blocked mutations must fail honestly; never
simulate success". The card's expectation ("12 эффектов доступны") is satisfied at the
level it can be honestly satisfied: the set of 12 is present and observable, the one
runtime-proven mode applies, the rest are honestly disabled **and not hidden**.
Recorded as a capability-layer finding (below), not fixed here.

Corroborating evidence: `/sys/class/leds/asus::kbd_backlight/kbd_rgb_mode` reads back
`EIO` on this unit — the WMI RGB-command attribute is write-only here.

## aura-live-only — PASS

Proof is from the running application over HEAD source, not from a picture, demo data or
an unrun binary:

* the process under test was started (`./target/release/orbis-control`, PID 1868447) and
  interacted with by real pointer input on DISPLAY=:0;
* every visual reading was cross-checked against an independent runtime source (asusd
  D-Bus properties), and the two agree;
* offscreen renders were not used as a substitute: no `ui_snapshot.rs` demo value was
  touched (`git diff` on that file is empty), so the defect-masking path the card forbids
  was not exercised.

## Hardware restored to the pre-probe baseline

The previous run left the hardware in probe values (`0 0 56 201 255 0 0 0 "High" "Right"`).
Restored to the baseline recorded in comment 31 (colour1 `(0,246,255)`, colour2
`(0,0,0)`, speed `"Med"`, `LedMode 0`) and confirmed by read-back:

```
$ busctl --system set-property xyz.ljones.Asusd /xyz/ljones/aura/tuf xyz.ljones.Aura \
    LedModeData "(uu(yyy)(yyy)ss)" 0 0 0 246 255 0 0 0 Med Right
set-property exit=0

--- restore-readback 2026-09-22T14:49:44+03:00
LedMode      = u 0
LedModeData  = (uu(yyy)(yyy)ss) 0 0 0 246 255 0 0 0 "Med" "Right"
Brightness   = u 3
```

Final confirmation after the restore (`live-17-resynced.png`, 14:50:12): hardware read-back is
still `0 0 0 246 255 0 0 0 "Med" "Right"` — the restore held.

**Observation, resolved as expected behaviour — not a defect.** After the external
restore the rendered badges still showed the probe values (red `229` / green `129` /
blue `139`) rather than the restored baseline `0,246,255`. I then drove a real navigation
away and back (Питание → Подсветка) and captured `live-20-after-nav-reload.png`: the badges
still showed `229`/`129`/`139` and Вторичный `133`/`130`/`135` unchanged.

Root cause, verified in source: the extra/Aura panel has **no periodic refresh**. The only
triggers for `extra_backend::refresh()` are (a) a completed mutation
(`request_aura_effect` → `refresh(&window)`) and (b) the `on_reload_requested` callback
(`extra_backend.rs:263`), which is wired *exclusively* to the «Обновить» button on the
System page (`system.slint:317` → `main-window.slint:422`). Nothing fires it on
navigation. The only `refresh_if_due` timer in the app belongs to
`quick_controls_backend` (`main.rs:1945`), not to this panel.

So the badges were stale only because *I* mutated the hardware out-of-band with `busctl`
— an external write the UI never observes. Within the card's own contract (drag a slider →
value applies → change is observable) the panel re-reads after every mutation, which is
exactly what the trace shows. This is worth noting as a design observation for the
UX/refresh surface (an externally-changed Aura state is not reflected until a mutation or
an explicit Обновить on the System page), but it is **not** an `SC-VISUAL-STATES` failure
and was not treated as one.

## Findings for other surfaces (not fixed here — card is narrow)

1. **Capability layer.** `SupportedBasicModes = au 0` (Static only) on ASUS TUF Gaming
   A17 FA707NV. If this machine supports more basic modes than asusd advertises, that is a
   capability/provider-surface question, not a UI defect. Recorded, not touched.
2. **`aura_effect_index` mapping** (`extra_backend.rs:1085-1093`) maps only `Static`,
   `Breathe`, `RainbowCycle` and returns `-1` for the other nine modes. Given
   `SupportedBasicModes = au 0` only `Static` is reachable here, so this is latent, not
   observable on this hardware. Noted for the capability/provider surface.
3. **`aura_observed` hardcodes `false`** at `supported_modes` index 9
   (`extra_backend.rs:1005`) — index 9 is not a wire mode (`AuraMode` jumps 8 → 10), so
   the entry exists only to keep the 13-slot array aligned with the UI's index-based
   lookups. Behaviourally consistent with `SupportedBasicModes`; flagged as a
   readability/latent-mapping note, not a defect observed live.

## Evidence index

| Path | What it shows |
|---|---|
| `aura-trace.txt` | timestamped asusd `LedMode`/`LedModeData`/`Brightness` read-back before and after each interaction, plus the restore |
| `live-00-start.png` | running application, initial page |
| `live-01-backlight.png` | Подсветка page with the Aura card |
| `live-04-preset-red.png` | «Красный» preset applied |
| `live-06-red-drag.png` | red slider dragged |
| `live-07-color-sliders.png` | all colour slider groups after the sweep |
| `live-09-speed-fast.png` | speed «Fast» selected |
| `live-10-breathe-disabled.png` | click on disabled `Breathe` → no hardware change |
| `live-11-secondary-blue.png` | secondary-blue applied, read-back `... 0 130 135` |
| `live-12-red-drag-mid.png` | **mid-drag** frame: preview follows the pointer |
| `live-13-recovered.png` | settled state after the drag resolved |
| `live-16-secondary-red.png` | secondary-red applied, read-back `133 130 135` |
| `live-17-resynced.png` | restored baseline + UI re-sync |
| `live-20-after-nav-reload.png` | navigation away/back: badges unchanged (no periodic refresh — see above) |
| `measure-effects.txt` | per-button rendered-state classification (12 effects + 3 speeds) |
| `measure-sliders.txt` | per-track fill/track measurements |
| `crop-effects-12.png` | crop of the effect grid |

Helpers (gitignored, `target/`): `drive.sh`, `uinput_mouse.py`, `aura-live.sh`,
`aurabuttons.py`, `effectstates.py`, `sliders.py`, `boxprobe.py`, `markers.py`.
