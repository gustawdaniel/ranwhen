class Ranwhen < Formula
  desc "Visualize when your system was running (with native macOS screen/power tracking)"
  homepage "https://github.com/gustawdaniel/ranwhen"
  url "https://github.com/gustawdaniel/ranwhen/archive/refs/tags/v0.2.1.tar.gz"
  sha256 "ff09b46d0fcdd83a19eca2404d0c62f89203f81069a5955a552c22581690f895"
  license "GPL-3.0-or-later"
  head "https://github.com/gustawdaniel/ranwhen.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  def post_install
    system "#{bin}/ranwhen", "--install-daemon"
  end

  test do
    system "#{bin}/ranwhen", "--help"
  end
end
