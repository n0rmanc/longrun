class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.9/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "0aa71930c20167d95f110954be418e2a5f7b53404bd30833c18b78339a112dd3"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.9/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "62f0aba9803ef2a5d2f4603e4f2efa2522dd8675ec21f86c7e97572202e8eb05"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.9/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "9edec7028752e275716196466e375ad94dab3f41b2285c57379810c0a89acf47"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
