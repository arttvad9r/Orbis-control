# Support Matrix Schema

The support matrix records evidence about a specific ASUS model and capability. It is documentation/release evidence, not runtime capability detection logic.

The canonical machine-readable contract is `docs/support-matrix.schema.json` (schema version 1).

## Current repository state

The repository currently ships only the schema plus `empty` / `unknown` example fixtures. There is no generated model table whose `supported` values should be treated as current product truth. Any future real model matrix must be added as an explicit evidence artifact and validated against this contract.

## Required structure

Each model entry contains its exact evidence-facing model identifier and a list of capabilities. Each capability records read and write support separately:

| Field | Meaning |
|---|---|
| `model` | Exact model identifier attached to the evidence record. |
| `capabilities[].id` | Stable capability identifier used only for the matrix/documentation contract. |
| `read.status` | `supported`, `unsupported`, `unknown`, or `not_applicable`. |
| `write.status` | Independent write status using the same enum. |
| `read.evidence[]` / `write.evidence[]` | Evidence records using the release evidence taxonomy. |
| `evidence.level` | `IMPLEMENTED`, `TESTED`, `PACKAGED`, `LIVE-VALIDATED`, `BLOCKED`, or `UNKNOWN`. |
| `evidence.references` | Auditable PR/commit/test/report/live-validation references. At least one reference is required for every evidence record. |

Read support never implies write support. A capability may therefore be read `supported` and write `unknown` or `unsupported`.

## Evidence rules

1. Use only evidence actually tied to the named model/capability/access direction.
2. `IMPLEMENTED`, `TESTED`, and `PACKAGED` do not imply `LIVE-VALIDATED`.
3. A structural probe or capability declaration is not write-success evidence.
4. `supported` requires positive evidence for the specific read/write direction. If that evidence does not exist, use `unknown` rather than inference.
5. `unsupported` requires evidence that the operation/capability is unavailable for the recorded model/environment; absence of evidence alone is `unknown`.
6. `BLOCKED` evidence names the concrete blocker in `note`. `UNKNOWN` is used when evidence is simply insufficient.
7. Evidence records remain revision/environment scoped. Do not generalize one model's live result to another model.
8. Product implementation presence is not evidence of hardware write support. A typed but deliberately disabled Orbis backend does not make a matrix write entry `supported`.

## Runtime and product-policy boundary

The support matrix must not be imported as model-name runtime inference. Production runtime support continues to come from typed probes/provider evidence and capability semantics. A row such as `FA707NV -> panel_miniled unsupported` is a documentation/acceptance fact only; it must not become a model-string conditional in production code.

For the current product composition, a deliberately disabled/unvalidated write is represented by an effective non-writable runtime status such as `Unsupported` with a reason describing the product/backend block. Orbis does **not** introduce a generic `DisabledByPolicy` status merely because writer code exists: that label could incorrectly imply the underlying hardware write is already proven and only policy is stopping it. A first-class policy-block status should be added only if future evidence needs to distinguish a **proven supported hardware operation** from a separately imposed product policy.

If runtime probe/evidence later contradicts a documentation row, fix or revalidate the matrix; do not add a model-name override to force the matrix result.

## Markdown presentation

Human-readable support tables derived from machine-readable data should use these columns:

| Model | Capability | Read | Read evidence | Write | Write evidence | Notes |
|---|---|---|---|---|---|---|

Do not collapse read/write into a single `Supported` column, and do not shorten evidence to an unqualified check mark. `LIVE-VALIDATED` must remain visually distinct from unit/CI/package evidence.

## Example

The following is illustrative schema usage only and is not a hardware claim:

```json
{
  "schema_version": 1,
  "models": [
    {
      "model": "EXAMPLE-MODEL",
      "capabilities": [
        {
          "id": "example_capability",
          "read": {
            "status": "supported",
            "evidence": [
              {
                "level": "TESTED",
                "references": ["PR #NN targeted test"]
              }
            ]
          },
          "write": {
            "status": "unknown",
            "evidence": [
              {
                "level": "UNKNOWN",
                "references": ["No live write evidence recorded"]
              }
            ]
          }
        }
      ]
    }
  ]
}
```
