# Contributing

Contributions are welcome, especially hardware reports from ASUS models beyond the primary FA707NV test machine.

## Development

The project uses the Rust toolchain selected by `rust-toolchain.toml`. See `README.md` for Arch dependencies.

For a normal task run:

```bash
scripts/verify task
```

Use `scripts/verify crate <affected-crate>` for a focused iteration and `scripts/verify full` for release-oriented validation.

## Hardware-control rules

Changes that write hardware state must preserve these invariants:

- expose a control only when the running system reports the capability;
- use the narrow typed D-Bus service instead of adding a generic privileged shell/sysfs proxy;
- treat a write as applied only after a successful read-back when the interface supports it;
- do not blindly retry a write whose outcome is unknown;
- keep polkit permissions as narrow as the operation allows.

If you are adding support for a new laptop, include model/kernel/asusd details and diagnostics that do not contain private user data.

## UI changes

Refresh the maintained screenshots with:

```bash
scripts/update-ui-screenshots.sh
```

Keep pull requests focused and include the verification commands actually run.
