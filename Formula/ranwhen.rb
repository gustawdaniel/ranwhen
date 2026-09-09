class Ranwhen < Formula
  desc "Visualize when your system was running (with native macOS screen/power tracking)"
  homepage "https://github.com/gustawdaniel/ranwhen"
  url "https://github.com/gustawdaniel/ranwhen/archive/refs/tags/v0.2.0.tar.gz"
  license "GPL-3.0-or-later"

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
