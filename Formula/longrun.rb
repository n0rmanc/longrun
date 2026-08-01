class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.2/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "f7ffb583adfa750c1d1df268387599084aa15e27a8224ca7830f940742d12a6b"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.2/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "295d1e7ef6c1956cad9f210d317d5b5e49df87194acef09e041367331ec3f8ab"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.2/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "9f79bd74f267a2e66238269ab0dd652903fc122cfbec7c8dd112910d24006fe6"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
