# ASUS FA707NV Hardware Baseline

> Read-only evidence collected 2026-08-21 on branch
> `asus-hardware-validation-20260821`; capability audit updated after
> `3791863`; platform-profile evidence model prepared after this snapshot.
> No hardware mutation commands were executed.

## Machine Identity

- vendor: ASUSTeK COMPUTER INC.
- model: ASUS TUF Gaming A17 FA707NV_FA707NV
- board: FA707NV
- BIOS: FA707NV.316 (2024-11-04)
- firmware version: DMI product version `1.0`; BIOS version above

## Operating System

- distro: NixOS 26.11 (Zokor)
- kernel: 7.1.8
- architecture: x86_64
- desktop environment: KDE Plasma (`XDG_CURRENT_DESKTOP=KDE`)
- display server: Wayland (`wayland-0`; Xwayland display `:0` also present)

## CPU

- model: AMD Ryzen 5 7535HS with Radeon Graphics
- cores/threads: 6 cores / 12 logical CPUs
- frequency information: min 416.0930 MHz, max 4604.7568 MHz; boost enabled
- RAM: 30 GiB visible to the running system

## GPU

- integrated GPU: AMD integrated graphics, PCI `1002:1681`, `amdgpu`
- discrete GPU: NVIDIA GeForce RTX 4060 Laptop GPU, PCI `10de:28e0`
- driver: NVIDIA `610.57.04` for the discrete GPU; `amdgpu` for the integrated GPU
- available management interfaces: DRM devices `card1`/`card2`, `nvidia-smi`, kernel
  runtime power paths; `glxinfo` and `vulkaninfo` were not installed
- `lspci` was not installed, so PCI identity was read from DRM/sysfs and NVIDIA
  query output

## ASUS / Platform Backend

- asusd service: active/running; D-Bus name `xyz.ljones.Asusd` present
- asusctl availability: installed, version 6.3.8
- platform_profile: readable; current value `balanced`
- available profiles: `quiet`, `balanced`, `performance`
- asusctl profile read: active `Balanced`; AC `Balanced`; battery `Quiet`
- profile write capability: not claimed; no write was attempted
- `asusctl fan-curve --help` exposes curve commands, but no curve read command was
  executed because the command surface also contains mutation options

## Graphics Switching

- supergfxd: inactive; no running service/backend evidence
- switcheroo-control: inactive; no running service/backend evidence
- kernel GPU interfaces: DRM `card1` uses `nvidia`; DRM `card2` uses `amdgpu`;
  per-device runtime power paths are present
- `vga_switcheroo` driver directory: absent

## Power

- UPower: active/running; `upowerd` version 1.91.3
- battery devices: `BAT1` / UPower `battery_BAT1`; ASUS A32-K55
- charge state: fully charged; 100%; ACAD online
- capacity: 78.5704 Wh full, 87.2952% of design capacity
- charge thresholds: UPower reports start 75%, end 80%, threshold support present
- direct effective threshold reads: `BAT1/charge_control_end_threshold` and the
  asusd `ChargeControlEndThreshold` property both report 100%; the UPower 80%
  value is therefore not treated as the mutation-owned configured value
- power_supply interfaces: `ACAD`, `BAT1`, and two UCSI USB source supplies

## Sensors

- hwmon devices: `hwmon0` through `hwmon13` present
- thermal zones: `acpitz` at 50.0 C and `iwlwifi_1` at 45.0 C
- fan sensors: ASUS hwmon exposes two channels: `cpu_fan` 2500 RPM and `gpu_fan`
  2400 RPM
- CPU temperature: `k10temp` Tctl 53.75 C
- GPU temperature: NVIDIA 39 C from `nvidia-smi`; AMDGPU edge 43.0 C
- additional read-only hwmon observations include NVMe, memory, Wi-Fi and UCSI
  sensors
- `asus_custom_fan_curve` hwmon exists but exposes no `fan*_input` or fan label
  in this snapshot

## Orbis Backend Discovery

### Available read providers

- kernel `platform_profile`: current value and choices readable
- ASUS profile observation through `asusctl`/asusd
- NVIDIA telemetry through `nvidia-smi`
- DRM/sysfs GPU identity and driver links
- sysfs `hwmon`, thermal-zone and `power_supply` reads
- UPower battery and line-power reads

### Unavailable providers

- Session1: **Unavailable**
  - Reason: missing dependency — `orbis-sessiond` user service is inactive and no
    Session1 service was observed
- Hardware1: **Unavailable**
  - Reason: missing dependency — no `io.github.orbiscontrol.Hardware` system-bus
    name was observed
- hardwared: **Unavailable**
  - Reason: backend unavailable — `orbis-hardwared` system and user services are
    inactive
- supergfxd provider: **Unavailable**
  - Reason: backend unavailable — `supergfxd` service is inactive
- switcheroo-control provider: **Unavailable**
  - Reason: backend unavailable — `switcheroo-control` service is inactive
- Orbis fan-curve provider: **Unavailable**
  - Reason: missing dependency — Session1/Hardware1/hardwared runtime path is not
    active; fan RPM read evidence does not prove curve write support
- Orbis mutation providers: **Unavailable**
  - Reason: permission/ownership boundary unavailable — Hardware1/hardwared was not
    running; no mutation was attempted

## Capability Validation Status

This matrix separates host observations from the Orbis capability registry. The
registry was not observed live: `orbisctl status --json` built successfully but
all Session1 observations were unavailable because the service was not active.

| Feature | Read | Write | Evidence | Confidence |
| --- | --- | --- | --- | --- |
| Performance / platform profile | Host read confirmed; Orbis read unavailable | Not promoted; Hardware1 owner unavailable | `platform_profile=balanced`; choices `quiet`, `balanced`, `performance` | Read: high; write: none |
| Fan telemetry | CPU/GPU RPM read confirmed | Not applicable from RPM evidence | ASUS hwmon: `cpu_fan=2500`, `gpu_fan=2400`; telemetry percent is absent | High for read |
| FanCurves | Not confirmed on this host; curve provider unavailable | Not promoted | RPM readings do not prove curve support; current registry requires typed curve evidence | None |
| Battery telemetry | Host read confirmed | Not applicable | UPower/sysfs reports BAT1 at 100% and AC online | High for read |
| ChargeLimit | Host sources disagree on reported/configured/effective facets; Orbis read unavailable | Not promoted; sessiond is read-only and Hardware1 is unavailable | UPower reports 80%; asusd and effective sysfs threshold report 100% | Read: medium, requires live Orbis read; write: none |
| GPU telemetry/identity | AMDGPU and NVIDIA identity/telemetry read confirmed | Not promoted | DRM/sysfs links and `nvidia-smi` | High for read |
| GpuMux / GpuPower / GpuAccess | Orbis reads unavailable | Not promoted | No supergfxd, switcheroo-control, Session1, or Hardware1 owner | None |

### Platform Profile validation matrix

The three-button product model is `quiet / balanced / performance`. The
backend/domain mapping is `Silent / Balanced / Turbo` respectively; no source
is allowed to select one silently when independent observations disagree.

| Feature | Read | Write | Evidence | Confidence |
| --- | --- | --- | --- | --- |
| Current profile | Supported when Session1 reads kernel `platform_profile` | Unknown | Session1 → `orbis-sessiond` → kernel sysfs; current `balanced` was observed on the host | High for host read; Orbis runtime unavailable in this snapshot |
| Available choices | Supported when kernel `platform_profile_choices` is readable | Unknown | Kernel sysfs choices: `quiet`, `balanced`, `performance` | High for host read |
| ASUS/asusd current profile | Not part of the current Orbis Performance provider flow | Unknown | Host `asusctl` observed `Balanced`; no independent Orbis asusd profile provider exists | Host-only evidence |
| Profile write | Unknown until validated | Unknown | Intended owner is original caller → Hardware1 → polkit → hardwared typed kernel writer; no write was attempted | None |
| Profile read-back | Unknown until a validated transaction exists | N/A | Intended authoritative read-back is a fresh kernel `platform_profile` read | None |
| Source disagreement | `Conflicted` | Unknown | `asusd != sysfs` must remain conflict evidence; neither source is selected silently | Fail-closed |

Desired, Observed and Pending are independent. A desired profile different from
the observed profile is represented as Pending only; this validation stage does
not reconcile or apply it.

The source-level tests already cover the relevant fail-closed rules: fan RPM
does not imply curve support, and missing mutation status does not promote a
write capability. No new hardware-facing test or mutation path was added.

## Safety Boundary

The snapshot contains read-only observations only. No profile set, fan-curve change,
GPU switch, power-limit change, EC write, keyboard write or Aura write was performed.
