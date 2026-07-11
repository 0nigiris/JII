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
  version "0.1.5-beta"
  license "GPL-3.0-or-later"

  on_linux do
    on_intel do
      url "https://github.com/0nigiris/JII/releases/download/v0.1.5-beta/jii-v0.1.5-beta-x86_64-linux.tar.gz"
      sha256 "33b5be49f61c85c61a7cdf11fffc6a7cc7889c81c64a056c924cd4cbe4378201"
    end
    on_arm do
      url "https://github.com/0nigiris/JII/releases/download/v0.1.5-beta/jii-v0.1.5-beta-aarch64-linux.tar.gz"
      sha256 "1844096e6f57552cd236d39a4518670e51fafce8ac26d657513756b543cfd424"
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
