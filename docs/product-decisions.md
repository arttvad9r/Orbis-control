# Product decisions

discovery_status: APPROVED
platform: DESKTOP
project_mode: EXISTING
delivery_target: COMPLETE_SMALL_PRODUCT
application_language: ru
reference_authority: TARGET_DESIGN
placeholder_policy: FORBIDDEN
asset_source: PROJECT_OWN
core_interaction: navigate existing Orbis sections and operate their visible controls

## Required surfaces

- Dashboard
- Performance
- Cooling
- Power
- Graphics
- Backlight
- Display
- System
- Settings

## Locked user decisions

- The target is the existing Orbis Control desktop application in Russian, not a separate prototype.
- The ten preserved screenshots are TARGET_DESIGN evidence; close adaptation is required, pixel equality is not.
- Do not add `Profiles` or `Devices` merely because they appear in a reference.
- Screenshots are visual evidence only; do not ship them or ASUS/ROG branding assets.
- Existing backend-wired controls retain real backend/read-back, unsupported, read-only, pending and error behavior.
- Explicitly approved exception to the forbidden-placeholder policy: future-only visual controls may show local press, selection or toggle feedback without a backend callback, persistence or hardware effect. They reset on page navigation or restart, show no disclaimer such as «к устройству не применено», and never claim hardware success.

## Definition of finished semantics

- Every visible in-scope existing control retains approved operational behavior or an honest existing unavailable/read-only state.
- The explicitly approved future-only visual exception is limited to local inert feedback described above; no `coming soon` or unfinished copy is shown.
- All required surfaces are coherent, readable and visually complete at 980×680 and 1200×800 in dark and light modes.
- Reference-driven visual requirements are acceptance requirements, including reference-to-actual comparison and material-deviation reporting.
- No clipping, overlap, unreachable content or known user-facing defect is accepted as complete.

## Open material decisions

NONE
