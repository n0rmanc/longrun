class Longrun < Formula
  desc "Run finite, long-running commands without model polling"
  homepage "https://github.com/n0rmanc/longrun"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.2.0/longrun-aarch64-apple-darwin.tar.gz"
      sha256 "84d911db6b87ecea5cffa3257293f5827f22d0d17ab5f5734d71feb1bcbe0deb"
    end

    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.2.0/longrun-x86_64-apple-darwin.tar.gz"
      sha256 "94524ee8f5add013206e450105f2b46e5f6b6c7a248fd40e04bde87ea68f731c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/n0rmanc/longrun/releases/download/v0.2.0/longrun-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "62841b085f24442a06cd3c461122dfa25bfd3362f21d3244f7237e2a7f29d11e"
    end
  end

  def install
    bin.install "longrun"
  end

  test do
    assert_match "longrun #{version}", shell_output("#{bin}/longrun --version")
  end
end
