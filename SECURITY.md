# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately through a
[GitHub Security Advisory](https://github.com/cole-robertson/inertia-rust-starter-kit/security/advisories/new)
on this repository, not in a public issue or pull request. Include the affected version or
commit, the steps to reproduce, and the impact you expect.

You'll get a reply on the advisory. Once a fix is released, the advisory is published with credit
to you unless you'd rather stay anonymous.

## Supported versions

Fixes land on `main` and in the next release. Apps generated from the kit don't update
themselves: watch the repository's releases and advisories, and apply fixes to your copy.

## How the kit is hardened

[docs/INERTIA_SECURITY.md](docs/INERTIA_SECURITY.md) describes the Inertia adapter's security
layers: cookies and keys, flash, redirects, CSRF, security headers and Precognition.
