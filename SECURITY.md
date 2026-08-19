# Security Policy

## Supported versions

Only the latest released version of Ketikin is supported. Security fixes are shipped in a new
release rather than backported to older ones, so if you are running an older build, updating to
the current release is the fix.

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Anything older | No |

You can find the current release on the
[Releases page](https://github.com/rendyuwu/ketikin/releases).

## Reporting a vulnerability

Please report security issues privately, not in a public issue.

Use **GitHub Security Advisories** on the repository:

1. Go to <https://github.com/rendyuwu/ketikin/security/advisories/new>.
2. Describe the issue, the version and platform you found it on, and the steps to reproduce it.
3. Include a proof of concept if you have one — it makes the report much faster to confirm.

### What to expect

- **Acknowledgement within 7 days.** If you have not heard anything in a week, feel free to nudge
  by opening a public issue that says only that you are waiting on a security report — no details.
- **An assessment within 30 days**, covering whether the issue is confirmed, how it is rated, and
  the rough plan for a fix.
- A fix released as soon as it is ready. The advisory is published once a fixed release is
  available, and you will be credited unless you would rather not be.

Ketikin is maintained by one person as an open-source project. There is no bug bounty, and
response times are best-effort — but reports are taken seriously and will not be ignored.

### Out of scope

Ketikin's entire purpose is to send synthetic keystrokes to the focused window, so that behaviour
by itself is not a vulnerability. Reports along the lines of "Ketikin can type into another
application" describe the feature. The same goes for the fact that templates are stored on disk in
plaintext — that is documented behaviour, and the README tells users not to save secrets as
templates. Likewise, a template store that another user of a shared machine can read or modify is
a consequence of where the operating system permits Ketikin to write; the README documents which
locations can be shared, and Settings > Storage reports when the resolved one is.

Things that *are* in scope include: bypassing update signature verification, getting Ketikin to
execute or load untrusted code, writing outside its resolved storage directory, or leaking the
contents of the type buffer somewhere it is not supposed to go.

## Updater trust model

Ketikin's auto-update mechanism is the most security-sensitive part of the application, since it
installs code. Here is how it is protected.

- Update artifacts are signed with [minisign](https://jedisct1.github.io/minisign/). Each release
  file has a matching detached `.sig`, produced by a private key that never leaves the maintainer's
  control and is held only as a GitHub Actions secret for the release workflow.
- The corresponding **public key is compiled into the Ketikin binary**. It is not fetched at
  runtime and cannot be replaced by the update itself, by a configuration file, or by anything on
  the network.
- Before an update is installed, its signature is verified against that embedded public key. An
  artifact that fails verification is discarded and never executed. A tampered release file, a
  hijacked download, or a malicious mirror therefore cannot install code — the signature will not
  validate.
- Automatic update checks can be disabled entirely in Settings. With them off, Ketikin does not
  contact GitHub on its own.

**If the signing key were ever compromised**, the response would be to generate a new keypair, ship
a new release built with the new key embedded, and announce the rotation prominently in this
repository — in the release notes, the changelog, and a pinned issue. Because the public key is
compiled in, users of older builds would need to download that release manually rather than
receiving it through the updater. Any such announcement will only ever come from this repository;
treat instructions to install Ketikin from anywhere else as untrustworthy.

## Responsible use

Ketikin types into whichever window has focus. Do not use it to put input into systems you are not
authorized to use, and be careful with secrets — the countdown before typing exists so you can
confirm the right window is focused before anything is sent.
