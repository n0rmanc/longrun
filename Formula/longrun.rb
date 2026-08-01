class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.7/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "e6eb70cdeccad402c9c5c8c1cb7c5444fd08d8aaf7cbe970028c8c009b42d5bb"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.7/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "efb3c284f60761ad826bc38e3a9cb3a445e840aa0efb2a7feeb66fb099e4b23a"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.1.7/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a487a1190b0f8a69747639a0c81ce208c34ad38be1d7ec751472c5196314402c"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
