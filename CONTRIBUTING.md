# Contributing to Git AI

Thank you for your interest in contributing to `git-ai`. This is a cool moment for the industry and we're all here to build ~~a~~ the standard for tracking AI code. 

## Getting Started

### Prerequisites

- Rust https://rustup.rs/ (compiler and tooling)
- GNU Make 4.4.1 or newer

On macOS, install the current GNU Make with Homebrew and put its unprefixed command first on `PATH`:

```bash
brew install make
export PATH="$(brew --prefix make)/libexec/gnubin:$PATH"
```

On Windows, install the pinned baseline with Chocolatey:

```powershell
choco install make --version=4.4.1
```

On Linux, use your distribution package when it provides GNU Make 4.4.1 or newer; otherwise install the current release from GNU.

On Windows, use a native PowerShell or Windows Terminal session for setup. If you are working in WSL, follow the Mac/Linux shell examples instead.

### Development Setup

1. **Fork the repository** on GitHub

2. **Clone your fork**:
   ```bash
   git clone https://github.com/YOUR_USERNAME/git-ai.git
   cd git-ai
   ```

3. **Build the project**:
   ```bash
   make build
   ```

4. **Run the tests**:
   ```bash
   make test
   ```

### Using a development build locally

It's often helpful to point your `git-ai` to a development build. The dev script builds the binary and installs it to `~/.git-ai/bin/git-ai`, replacing the production binary so you can test changes with real git repositories.

```bash
make dev
```

If `~/.git-ai` isn't set up yet, the script will run the installer automatically first.

On Windows, `make dev` runs the Windows development script, installs the current branch build, updates the `git.exe` shim, runs install hooks, and restarts the daemon.

Use `make dev` before testing Windows behavior locally. `cargo build` only produces `target\debug\git-ai.exe`; it does not replace the installed `git-ai.exe`, update the `git.exe` shim, run install hooks, or restart the daemon. Testing setup, daemon startup, or git proxy behavior against only `target\debug\git-ai.exe` can give misleading results.

For changes that affect Windows setup or daemon startup, verify the installed development build in a fresh daemon session:

```powershell
# 1. Install the current branch build first.
make dev

# 2. Stop any daemon that was already running.
~\.git-ai\bin\git-ai.exe bg shutdown --hard

# 3. Trigger daemon startup through the installed git-ai.
git-ai status

# 4. Confirm the daemon is running.
git-ai bg status
Get-Process git-ai -ErrorAction SilentlyContinue
```

## Contributing Changes

### Before You Start

- **Check existing issues**: Look for related issues or feature requests
- **For new features or architectural changes**: We encourage you to chat with the core maintainers first to discuss your approach. This helps ensure your contribution aligns with the project's direction and saves you time.

### Submitting a Pull Request

1. Create a new branch for your changes:
   ```bash
   git checkout -b my-feature-branch
   ```

2. Make your changes and commit them with clear, descriptive messages

3. Push to your fork:
   ```bash
   git push origin my-feature-branch
   ```

4. Open a Pull Request against the main repository

5. **Reference any related issues** in your PR description (e.g., "Fixes #123" or "Related to #456")

6. Wait for review from the maintainers

## Code Style

The project uses standard Rust formatting. Please run `make fmt` and `make lint` before committing your changes.


## Getting Help

For questions about this fork, open a
[GitHub issue](https://github.com/woud420/git-ai/issues). Include the platform,
the command you ran, and enough reproduction detail for another contributor to
verify the behavior.
