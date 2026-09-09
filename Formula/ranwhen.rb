class Ranwhen < Formula
  desc "Visualize when your system was running (with native macOS screen/power tracking)"
  homepage "https://github.com/gustawdaniel/ranwhen"
  url "https://github.com/gustawdaniel/ranwhen/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "7ca51a3e9c5538178a451d80f1886cbe3a9a753e1a3feef940c1aa4e9feb1312"
  license "GPL-3.0-or-later"
  head "https://github.com/gustawdaniel/ranwhen.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
    bin.install_symlink "ranwhen" => "runwhen"
  end

  def post_install
    system "#{bin}/ranwhen", "--install-daemon"
  end

  test do
    system "#{bin}/ranwhen", "--help"
  end
end
