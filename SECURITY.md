# Security Policy

## Scope & design

Aletheia is a **local, read-mostly privacy-audit tool**. By design it:

- Reads **only the profiles of the user who runs it**, on the local machine.
- **Never reads, decrypts, or transmits** cookie values or passwords. It reports
  *which* sites have sessions/credentials and cookie **names/metadata** only.
- Sends **nothing over the network**. There is no telemetry and no remote
  server.
- Only writes when you explicitly ask it to (deleting cookies/passwords or
  clearing a cache directory), always behind a confirmation dialog. Cache
  deletion is restricted to directories literally named `Cache`/`cache2` under
  your home directory.

## Reporting a vulnerability

If you find a security issue (for example, a path that could read or modify data
outside the current user's own browser profiles, or any data leaving the
machine), please **do not open a public issue**. Instead, report it privately:

- Use GitHub's **"Report a vulnerability"** (Security → Advisories) on this
  repository, or
- Email the maintainer (see the repository profile).

Please include steps to reproduce and the affected OS/browser. We aim to
acknowledge reports within a few days.

## Supported versions

This is a young project; only the latest `main` is supported. Pin a tag if you
need stability.
