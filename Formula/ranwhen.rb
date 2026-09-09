class Ranwhen < Formula
  desc "Visualize when your system was running (with native macOS screen/power tracking)"
  homepage "https://github.com/gustawdaniel/ranwhen"
  url "https://github.com/gustawdaniel/ranwhen/archive/refs/tags/v0.2.2.tar.gz"
  sha256 "7a2dfdfcefc801e65cbc5d60604e1b87711a72eb2caf30ed18a1d87e92e71633"
  license "GPL-3.0-or-later"
  head "https://github.com/gustawdaniel/ranwhen.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      To enable automatic background activity tracking (preserving history across macOS log rotations):
        ranwhen --install-daemon

      To verify daemon status:
        ranwhen --status-daemon
    EOS
  end

  test do
    system "#{bin}/ranwhen", "--help"
  end
end
