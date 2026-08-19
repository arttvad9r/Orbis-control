# Release Evidence Taxonomy

This taxonomy defines the evidence labels used for release, beta-readiness, support-matrix, and current-state claims. It prevents implementation, tests, packaging, and live hardware observations from being treated as equivalent evidence.

## Labels

| Label | Required evidence | Does not imply |
|---|---|---|
| `IMPLEMENTED` | The claimed behavior exists in production code at a referenced commit/PR. | Tests passed, packaging works, or hardware behavior was observed. |
| `TESTED` | Relevant deterministic unit/integration/P2P tests passed for the referenced scope. | Packaged execution or live hardware behavior. |
| `PACKAGED` | The relevant packaged artifact was built and exercised in the target packaging/runtime environment. | Hardware semantics or device-specific support. |
| `LIVE-VALIDATED` | The exact claim was observed on live supported hardware with the model/environment and validation steps recorded. | Support on other models, firmware, kernels, or untested operations. |
| `BLOCKED` | A named prerequisite prevents the claim from advancing; the blocker and missing evidence are recorded. | Partial implementation is safe or production-ready. |
| `UNKNOWN` | Available evidence is insufficient to make a stronger claim. | Unsupported, broken, or absent. |

Positive evidence labels are cumulative only when each level has independent evidence. A claim may therefore be `IMPLEMENTED / TESTED` without being `PACKAGED` or `LIVE-VALIDATED`.

## Promotion rules

1. Never infer a stronger label from a weaker one.
2. Unit, integration, P2P, VM, or CI results may establish `TESTED`; they never establish `LIVE-VALIDATED`.
3. A successful package build or packaged smoke test may establish `PACKAGED`; it never establishes device-specific read/write correctness.
4. Hardware claims require `LIVE-VALIDATED` evidence for the exact operation being claimed. Read evidence does not prove write support, and one capability does not prove another.
5. `BLOCKED` names the concrete prerequisite and the evidence needed to remove the blocker. Use `UNKNOWN` when no concrete blocker is established.
6. Planned architecture, disabled/preview UI, mocks, snapshots, and capability declarations are not acceptance evidence for production behavior.
7. Evidence is scoped to the referenced revision. Later behavior-changing changes require revalidation of affected claims.

## Required evidence record

Every beta/release claim should record enough information to audit the label:

- claim and exact scope;
- evidence label(s);
- commit/PR or packaged artifact reference;
- validation command or procedure;
- result and date;
- environment for `PACKAGED` claims;
- hardware model plus relevant runtime/firmware context for `LIVE-VALIDATED` claims;
- known limitations or blocker when applicable.

For hardware-facing features, record read and write evidence separately. Do not collapse capability presence, readable state, writable state, accepted mutation, and authoritative applied/read-back state into one claim.

## Relationship to other status words

Terms such as `PARTIAL`, `READ-ONLY`, `MOCK-ONLY`, `SUPPORTED`, `UNAVAILABLE`, or `PREVIEW` describe product/runtime state; they do not replace these evidence labels. Release documentation should state the product/runtime status and then attach only the evidence labels actually proven.

## Completion rule

A roadmap item may be marked complete when implementation evidence exists, required tests are recorded, and any hardware-facing claim has the necessary live runtime/hardware evidence. Architecture intent alone is never completion evidence.
