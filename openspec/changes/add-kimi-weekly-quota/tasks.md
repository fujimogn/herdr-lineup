# Implementation tasks (pending owner decisions)

- [ ] Resolve the provenance and credential-channel Open questions in `design.md`;
      do not enable the worker without a verified account binding.
- [ ] Add the default-off Kimi-specific option to plugin config and document its
      conjunction with `allow_network` without changing user settings. Plan
      compatibility for older binaries because config rejects unknown keys.
- [ ] Add a bounded, passive Kimi quota worker and strict `limit7d` parser using
      recorded fixtures; keep credentials and account identifiers out of state,
      diagnostics, and logs.
- [ ] Add binding invalidation, cache expiry, failure categories, and
      no-cross-account publication tests, including cold-session non-resumption.
- [ ] Verify the installed Kimi version and experimental schema in an approved
      environment, then update user documentation and run release checks.

These are future implementation tasks. This change writes no executable code.
