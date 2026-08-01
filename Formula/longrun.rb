class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.3/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "a9cf6d4f0fbfa7ecec4378f4fb1fc62b0317714b25f8e53c33f5f4e067a22d8b"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.3/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "7b3874164a7c128e3f1378caea7724b6b5d3b089dd05fc8411fc7f88f3836f99"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.3/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1d10248dacb03777144e1e8065cf8f433626afb1df89923593ca58a5e4cd6eec"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
