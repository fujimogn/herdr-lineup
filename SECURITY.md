# Security

## Current scaffold

The executable only parses command-line arguments and prints help, version,
or a not-implemented message. It does not read agent sessions or configuration,
write files, connect to Herdr, or send data anywhere.

## Planned local data access

The planned usage readers will read only local agent session and configuration
files for the associated pane. These files may contain sensitive content;
readers must extract only the usage fields they need, leave the files unchanged,
and never log session contents or secrets. They will not upload file contents,
credentials, or usage data to remote services. No telemetry is planned.

The planned publisher will send display metadata only to the local Herdr socket.
Repository labels will use local Git refs without fetching. Plugin configuration
and state will live in Herdr's plugin directories. The only planned user-config
writes are the explicit, opt-in `configure` and `unconfigure` actions.

This is a design boundary, not an implemented security guarantee. Update this
document with exact paths, extracted fields, permissions, and limitations when
the readers and publisher are implemented. Provider integrations that require
remote requests need a separate design decision and updated documentation.

## Reporting a vulnerability

Use this repository's GitHub Security page to report a vulnerability privately
when private reporting is available. Otherwise open an issue asking for a
private contact channel, without including secrets or exploit details.
