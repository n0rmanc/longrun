class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.0/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "2b8b8eaca2493f31fd18248961bc0751c7ce19a9a54f55e4b0a39d51aee749f8"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.0/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "988627c409e06d23a971def614acd8c504c5dfb9bd9d515400afaeb818f8a805"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.0/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "247e37db51f000dc158b94a1aaac662457f67b97c6fb9ce6fcac689c99f908f1"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
