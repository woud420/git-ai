class GitAi < Formula
  desc "Track explicit AI authorship in Git repositories"
  homepage "https://github.com/woud420/git-ai"
  license "Apache-2.0"
  head "https://github.com/woud420/git-ai.git", branch: "main"

  depends_on "rustup" => :build
  depends_on "git"

  def install
    # Homebrew's Rust can lag the source minimum; keep downloads out of user state.
    ENV["CARGO_HOME"] = buildpath/".cargo"
    ENV["RUSTUP_HOME"] = buildpath/".rustup"
    ENV["RUSTUP_TOOLCHAIN"] = "1.100.0"
    ENV.prepend_path "PATH", Formula["rustup"].opt_bin
    system "rustup", "toolchain", "install", ENV.fetch("RUSTUP_TOOLCHAIN"),
           "--profile", "minimal", "--no-self-update"
    system "cargo", "install", *std_cargo_args, "--bin", "git-ai"
    (bin/"git-ai-package-manager").write "homebrew\n"
  end

  def caveats
    <<~EOS
      Enable per-user integration in a normal shell:
        git-ai install-hooks
        git-ai config --add allowed_repositories /path/to/repository

      After upgrading, run git-ai install-hooks again to refresh binary paths.
      Before brew uninstall, run git-ai uninstall to remove your integration.
    EOS
  end

  test do
    assert_match(/^\d+\.\d+\.\d+/, shell_output("#{bin}/git-ai --version"))
    assert_match "brew upgrade", shell_output("#{bin}/git-ai upgrade 2>&1", 1)
  end
end
