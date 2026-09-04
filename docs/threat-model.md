# Threat Model — Orbis Control

This document defines durable security assumptions and trust boundaries. It is not a feature-status ledger or release checklist.

## 1. Trust boundaries

```text
TL0  user session / orbis-ui / orbisctl
  │ typed user/session reads
  ▼
TL1  orbis-sessiond (unprivileged user daemon)
  │ read-only service/kernel access
  ▼
TL2  UPower / asusd / supergfxd / kernel and compositor APIs

Separate privileged mutation boundary:

TL0 original application caller
  │ typed system D-Bus Hardware1 request
  ▼
TL4 orbis-hardwared (root, sandboxed)
  │ capability-specific authorization + bounded backend
  ▼
mutation owner / fixed platform ABI
```

Critical rule: `orbis-sessiond` must never become a privileged mutation deputy. When a privileged operation requires caller authorization, `Hardware1` authorizes the original system-bus sender rather than trusting caller-provided identity data or a second daemon hop.

## 2. Assets

| Asset | Main risk |
|---|---|
| Hardware settings | unsafe/incorrect writes, thermal or stability impact |
| GPU/display lifecycle | loss of display/session, unexpected reboot/logout requirements |
| User preferences and Desired state | unintended automatic application |
| Capability evidence | falsely reporting unsupported hardware as writable/supported |
| Diagnostics/export | privacy leakage |
| D-Bus/system service input | malformed, spoofed or stale state |
| Root helper | privilege escalation or generic privileged-write primitive |

## 3. Privileged service requirements

`orbis-hardwared` may expose only narrow semantic operations required by the product.

Forbidden privileged designs include:

- caller-supplied arbitrary filesystem/sysfs paths;
- arbitrary shell commands or executables;
- generic D-Bus forwarding;
- caller-supplied UID/PID/session/executable metadata used as an authorization trust anchor;
- broad write access added merely because a backend is easier to implement that way.

Each privileged capability should have:

1. a precise user-visible meaning;
2. validated typed input;
3. capability-specific authorization;
4. the minimum backend access required for that operation;
5. explicit result semantics;
6. denial/unavailable/malformed/partial-failure tests;
7. controlled live validation when physical device behavior is part of the claim.

Systemd/polkit packaging should preserve least privilege: no unnecessary Linux capabilities, no broad filesystem write surface, no inactive/remote default authorization unless the product requirement explicitly justifies it.

## 4. Confused-deputy prevention

Forbidden mutation flow:

```text
GUI → Session1/sessiond → Hardware1
```

Required model:

```text
original application caller
→ Hardware1
→ authorization based on the original bus caller
→ bounded mutation
```

An unprivileged user-session service may aggregate reads, but it must not accidentally lend its identity/authority to another process for privileged mutation.

## 5. Capability/evidence integrity

A false `Supported`, writable or `Applied` claim is a security and product-trust failure.

Rules:

- model name alone is not runtime support evidence;
- object/file presence alone is not sufficient write evidence;
- validation success alone is not proof that a real mutation owner is usable;
- read evidence and write evidence remain separate;
- backend capability and product/policy permission remain separate;
- persisted Desired state is not Observed state;
- `Accepted` is not `Applied`;
- empty/partial telemetry is not proof of fresh useful state;
- unsupported or uncertain mutation paths fail closed.

## 6. Mutation outcome integrity

Never display requested state as authoritative physical state merely because a write call returned.

When possible, confirm the result through an authoritative read-back. When a transition inherently requires reboot/logout/later observation, expose an explicit pending state.

A transport timeout after a mutation may have been dispatched is an **unknown outcome**. Do not blindly retry such operations; doing so can duplicate a non-idempotent hardware action.

## 7. External service and kernel input

Even trusted host services/kernel interfaces are protocol inputs from Orbis' perspective. They may restart, change version, disappear, deny permission, return malformed values or conflict with another owner.

Mitigations:

- typed adapters and strict decode;
- bounded provider operations where hangs are plausible;
- capability-local failure instead of global fake fallback;
- authoritative reads where freshness matters;
- explicit unknown/unavailable states;
- no inference of one hardware concept from a different one unless the platform contract actually guarantees it.

## 8. Persistence and automation

Loading preferences/configuration is hardware-inert. Persisted intent must not mutate hardware simply because the application starts.

Separate concerns such as UI preferences, window state and hardware Desired state so one persistence mechanism cannot silently become an "apply everything" mechanism.

Any automation/reconciliation feature must pass through the same capability, authorization, mutation-result and unknown-outcome rules as a direct user action. Automation must not create an alternate privileged path.

## 9. Diagnostics and privacy

Diagnostics should be allowlist-based and read-only.

Do not export by default:

- device serial numbers or machine UUIDs;
- asset tags;
- arbitrary environment variables;
- arbitrary files or journals;
- credentials or secrets;
- full home-directory paths when a redacted representation is sufficient.

Diagnostics must not activate stopped privileged services solely to collect more data.

## 10. Mock/test isolation

Fixtures, mock providers and fake device state are useful for development but must never masquerade as production device evidence.

Normal automated tests should use private P2P D-Bus, fake sysfs/filesystems, fixtures and NixOS VMs rather than mutating the developer's real laptop.

Live hardware tests must be deliberate and opt-in.

## 11. Runtime availability

A stuck or missing backend must not indefinitely block unrelated application work when the operations are independent.

Provider calls that may hang should be bounded and report typed timeout/unavailable state. Retry policy is capability-specific; no generic layer may automatically retry a possibly-dispatched mutation.

## 12. Packaging and deployment

Declarative package/module source is authoritative for installed service files, D-Bus policy and polkit policy.

Development helpers must not silently overwrite package-manager/Nix-owned service configuration or compete for a production D-Bus name without making that ownership explicit.

A packaged daemon being present does not prove it is running or usable; capability discovery handles service absence honestly.

## 13. Security acceptance for a new privileged capability

Before enabling a new privileged mutation in the product:

1. define the exact hardware/user concept;
2. identify the real write owner and non-mutating support evidence;
3. define the smallest typed API;
4. validate every untrusted input at the owning boundary;
5. preserve original caller authorization;
6. define read-back, Pending, Accepted and unknown-outcome semantics;
7. test denied, unavailable, malformed and partial-failure paths;
8. verify packaging/sandbox policy grants only the required access;
9. perform controlled device-specific live validation if physical behavior is being claimed.

## 14. Assumptions

- the attacker does not already control root or the system bus;
- the kernel and installed system services are host TCB, though their APIs/data may be unavailable or incompatible from Orbis' perspective;
- arbitrary same-user processes may call user-accessible APIs;
- package/repository integrity and host policy configuration are trusted at installation time.

Changes to these assumptions or to the fundamental privilege boundary require an explicit security review/ADR. Ordinary product refactors within these boundaries do not.