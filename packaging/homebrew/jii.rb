# Homebrew formula for JII — prebuilt-binary (no compile).
#
# Serves Homebrew on Linux (a.k.a. Linuxbrew): it installs the static-musl binary from
# the GitHub release, so `brew install <tap>/jii` works on any Linux with Homebrew.
# macOS is not covered yet — JII is Linux-only today (it drives Linux package managers);
# a macOS bottle needs a native mac build first (tracked in packaging/README.md).
#
# Put this in a tap repo (e.g. github.com/0nigiris/homebrew-jii → Formula/jii.rb) and
# users run:  brew tap 0nigiris/jii && brew install jii
# On each release bump `version` and refresh both sha256s.
class Jii < Formula
  desc "Smart universal package installer for Linux"
  homepage "https://github.com/0nigiris/JII"
  version "0.1.20-beta"
  license "GPL-3.0-or-later"

  on_linux do
    on_intel do
      url "https://github.com/0nigiris/JII/releases/download/v0.1.20-beta/jii-v0.1.20-beta-x86_64-linux.tar.gz"
      sha256 "15efe4b219966ec592c14d24cf90636566c367e2d3e1afc37c660518fea6593f"
    end
    on_arm do
      url "https://github.com/0nigiris/JII/releases/download/v0.1.20-beta/jii-v0.1.20-beta-aarch64-linux.tar.gz"
      sha256 "51ddcba262c4d57d6c3cc0c88e6002b225dcbf554d39edb0d24fbd663a3e6322"
    end
  end

  def install
    bin.install "jii"
    man1.install "jii.1"
    bash_completion.install "completions/jii.bash" => "jii"
    zsh_completion.install "completions/_jii"
    fish_completion.install "completions/jii.fish"
  end

  test do
    assert_match "jii", shell_output("#{bin}/jii --version")
  end
end
