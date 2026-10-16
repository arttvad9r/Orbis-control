# Security policy

Orbis Control intentionally separates an unprivileged GUI from privileged hardware writes. Bugs that widen that boundary are security-sensitive.

## Reporting

Do not post exploit details, local policy bypasses, private diagnostics or destructive hardware-write sequences in a public issue. Use GitHub's private vulnerability-reporting channel when available; otherwise contact the repository owner through GitHub first and request a private channel.

## Security boundary

High-priority reports include:

- bypassing polkit authorization or invoking privileged methods without the intended policy;
- escaping the typed `orbis-hardwared` API into arbitrary root command/file access;
- unsafe path handling around sysfs or D-Bus inputs;
- capability detection exposing writes that the hardware/kernel path does not actually support;
- state reporting that marks a failed/unknown write as successfully applied;
- diagnostics exporting secrets or unrelated private user data.

Hardware tuning always carries platform risk. Security fixes should narrow authority and fail safely rather than silently applying guessed values.
