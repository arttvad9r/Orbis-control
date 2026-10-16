---
name: code-security
description: Security review guidance for application and infrastructure code. Use when code crosses trust boundaries, handles untrusted input, authentication/authorization, secrets, files, databases, networks, cryptography, privileged operations, CI/CD, containers, or when the user asks for a security review.
---

# Code security review

Treat security as boundary analysis, not a generic checklist. Preserve existing architecture and inspect the actual data/control flow before proposing changes.

## Workflow

1. Identify assets, actors, trust boundaries, entry points, privileged operations, persistence, and outbound connections.
2. Trace untrusted data from source to security-sensitive sinks.
3. Check authorization separately from authentication; enforce decisions server-side or at the privileged boundary.
4. Prefer structured APIs over string-built commands, queries, paths, HTML, URLs, or configuration.
5. Validate at the boundary with allowlists/ranges/schema where possible; encode for the destination context.
6. Keep secrets out of source, logs, URLs, crash reports, generated artifacts, and client-visible bundles.
7. Apply least privilege to filesystem, subprocesses, network, tokens, service accounts, containers, CI jobs, and agent tools.
8. Review dependencies and lockfiles for provenance, stale/high-risk packages, install scripts, and known vulnerabilities using project-native tooling when available.
9. Verify failure behavior: deny/fail closed for security decisions, avoid unsafe retries after ambiguous privileged mutations, and do not leak sensitive diagnostics.
10. Test the changed boundary with negative cases. Automated scanners are supporting evidence, not proof of safety.

## High-value review areas

- Injection: SQL/NoSQL, shell, template, expression, header/response, path traversal, SSRF and unsafe deserialization.
- Identity/access: session/token validation, object-level authorization, role/tenant separation, credential recovery and rate limits.
- Data: sensitive storage, transport security, logs/backups/exports, cache exposure and deletion semantics.
- Browser/API: XSS, CSRF where relevant, CORS, cookie attributes, redirects, upload/download handling and content types.
- Native/system: intents/deep links, IPC/D-Bus/binder boundaries, permissions, privileged helpers and hardware/system writes.
- Infrastructure: Docker, systemd, CI/CD, IaC, exposed ports, default credentials, broad capabilities and writable mounts.
- Supply chain: dependency confusion, unpinned actions/images, unexpected lifecycle hooks, vendored binaries and generated code.

For rules or platform details that may have changed, consult current authoritative OWASP/CWE/platform documentation rather than relying on remembered version-specific advice.

## Output

Report findings by severity and evidence: boundary/location, exploit or failure path, impact, and smallest robust fix. Distinguish confirmed findings from hardening suggestions and scanner-only signals.
