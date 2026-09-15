# Homebrew and Chocolatey

These packages install `git-ai` only. They do not install a `git` wrapper,
run agent setup, change Git configuration, or opt repositories into collection.
The package manager owns the executable. `git-ai upgrade` directs you to that
manager; automatic self-updates are disabled for these installations.

When switching from a script installation, first run `git-ai uninstall` as
your normal user and follow any remaining binary-removal instructions it
prints. Keep the default data-preserving mode. This prevents an older
`~/.git-ai/bin` executable from taking precedence over the package command.

## Homebrew: build from source

The repository itself is a tap. After this change is merged into `main`:

```bash
brew tap woud420/git-ai https://github.com/woud420/git-ai
brew install --HEAD woud420/git-ai/git-ai
```

Homebrew builds the current fork `main` with Rust and its locked dependencies.
This works before the first binary release and supports macOS and Linux on
x86_64 and ARM64. Homebrew 7 may require trusting the formula first:
`brew trust --formula woud420/git-ai/git-ai`.

In a normal user shell, enable integrations and choose a repository:

```bash
git-ai install-hooks
git-ai config --add allowed_repositories /path/to/repository
```

To upgrade the source installation:

```bash
brew update
brew upgrade --fetch-HEAD woud420/git-ai/git-ai
git-ai install-hooks
```

Run `install-hooks` after each upgrade: integrations store an absolute binary
path, and Homebrew moves each version to a different Cellar directory.

To remove it, first remove your user integrations, then the package:

```bash
git-ai uninstall
brew uninstall woud420/git-ai/git-ai
```

`git-ai uninstall` keeps configuration and attribution data by default and
leaves the Homebrew-owned executable for Homebrew to remove.

## Chocolatey: install a release package

The fork has no published release packages as of 2026-09-15. The release
workflow produces `git-ai.<version>.nupkg` for stable releases after this
change is merged. This is a local package source; there is no claim of a
Chocolatey Community Repository listing.

Download the package and `SHA256SUMS` from the same
[fork release](https://github.com/woud420/git-ai/releases). Compare the package's
`Get-FileHash -Algorithm SHA256` output with its entry in `SHA256SUMS`. Place
the verified package in a directory such as `C:\Packages\git-ai`, then run:

```powershell
choco install git-ai --source C:\Packages\git-ai --yes
```

The package bundles both Windows binaries and selects native x64 or ARM64;
32-bit Windows is unsupported. Install Git before enabling integrations.
The package checks the selected executable's SHA-256
before installation and creates only the `git-ai` command shim. Chocolatey's
normal administrator installation never configures that administrator's
agents or Git. Open a **non-elevated** shell for your own user and run:

```powershell
git-ai install-hooks
git-ai config --add allowed_repositories C:\work\my-repository
```

Before an upgrade, stop your daemon in that same user shell to release the
Windows executable lock:

```powershell
git-ai daemon shutdown
```

Download and verify the new `.nupkg` into the same local source directory,
then run the package-manager command with the privileges Chocolatey requires:

```powershell
choco upgrade git-ai --source C:\Packages\git-ai --yes
```

Run `git-ai install-hooks` again in your normal shell. Before removal, run
`git-ai uninstall` in that shell; then run `choco uninstall git-ai --yes`.
On shared computers, each user who enabled integrations must clean up their
own integrations and stop their daemon before a package upgrade or removal.
Neither Chocolatey nor Homebrew deletes your attribution databases.

## Generate release packages

The generator reuses the six platform executables and their `SHA256SUMS`
entries from the release build. It verifies every input before writing outputs.
For example, with the artifacts from an intended `v1.2.3` release in `release/`:

```bash
python3 packaging/managers/generate.py \
  --repository woud420/git-ai --version 1.2.3 --tag v1.2.3 \
  --assets-dir release --output-dir release
```

Outputs are `git-ai.rb` (version-pinned binary Homebrew formula) and
`git-ai.1.2.3.nupkg` (embedded Windows package). Stable production releases
include both files in release checksums and provenance attestations. Dry runs
produce reviewable workflow artifacts. Prerelease builds do not update stable
package definitions or publish packages to a registry.

After a stable release exists, a maintainer can promote its reviewed
`git-ai.rb` to `Formula/git-ai.rb` in a separate PR. Until then, the tracked
formula remains a source-only HEAD formula. Publishing a GitHub release does
not silently push a tap update or submit a Chocolatey community package.

## Verification

```bash
python3 -m unittest discover -s packaging/managers/tests -v
```

The package-manager workflow builds and installs the source formula on macOS
and tests Chocolatey install, upgrade, and uninstall on Windows. Its fixture
artifact names exercise packaging; release binaries still come from the
existing six-platform build matrix.

References: [Homebrew taps](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap),
[formula cookbook](https://docs.brew.sh/Formula-Cookbook), and
[Chocolatey package creation](https://docs.chocolatey.org/en-us/create/create-packages/).
