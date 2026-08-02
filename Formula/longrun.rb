class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.8/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "98f46678f35c87dc2f793f6565e810d0a7afdd2de68bba0ed9d31f4297c47e30"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.8/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "c789b72f5d926d0c6a25b76f6921e8c6731c9f0e3c24556276ac1b0bb3622480"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.8/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "282e4f0aa033d72280ea76f1dd4d10551c9127cea13e501114d8a9e8b2006e69"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
