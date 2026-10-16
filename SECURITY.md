# Security policy

## Reporting a vulnerability

Please report a vulnerability privately, not in a public issue:

- Use **Security → Report a vulnerability** on this repository (GitHub private vulnerability reporting).

Include what you found, how to reproduce it, and the version or commit it affects. I will reply as soon as I can and
will tell you when a fix is released.

## Supported versions

Only the latest release is supported. Fixes land on `main` and ship in the next release.

## Tokens and secrets

These apps never write a GitHub token or any other secret to a file; they read it from `gh auth token` or the OS
credential store. If you see one in a log, a crash message or a settings screen, please report it the same way.
