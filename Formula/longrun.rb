class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.4/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "8867e05a81402f7189d1f7d275612960b8bc42fde78b7b32bd1b9a70c4a259b6"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.4/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "780681645c4ff5d9c38456659b2270e75f8b719f70e5ef66b6c4c190ba9ad575"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.4/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "f04c7afaf2dd91690765027bd45a449096cf2f0f3f55e52b4a0194ca05162a18"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
