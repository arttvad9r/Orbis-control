# FA707NV Platform Profile Validation

## Device
ASUS TUF Gaming A17 FA707NV

## Backend
- sessiond
- hardwared
- Hardware1
- polkit

## Validation

### Initial
balanced

### Requested
performance/turbo

### Applied
turbo

### Read-back
turbo

### Restore
balanced

### Final
balanced

## Safety Notes
- only platform profile tested
- no fan writes
- no GPU switching
- no battery writes
- no automation
- restore verified

## Validation Conditions Met
- Hardware1 available: YES
- write capability == Supported: YES
- No fan/GPU/battery mutations
- Restoration to initial state confirmed

### Preconditions
- Both backend services active
- D-Bus names owned
- Platform profile evidence present
- Write capability proven before mutation
- Explicit APPLY-TEST confirmation provided
- Restoration to initial state verified

### Mutation Rules (not performed)
- Fan writes: STOPPED (not performed)
- GPU writes: STOPPED (not performed)
- Battery writes: STOPPED (not performed)
- Automation: STOPPED (not performed)
- Permissions: maintained throughout

## Device Under Test
ASUS TUF Gaming A17 FA707NV

## Backend Services
- **sessiond**: user-session D-Bus daemon, provides Session1 API
- **hardwared**: root system D-Bus service, provides Hardware1 typed API
- **Hardware1**: privileged mutation API via D-Bus + polkit
- **polkit**: per-capability authorization for Hardware1 mutations

## Validation Method
`orbisctl validate platform-profile --profile performance --apply-test`

Result: PASS (profile applied, verified, and restored)