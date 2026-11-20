#!/bin/sh
# Print the Homebrew formula (peetzweg/homebrew-tap, Formula/speak.rb) for a published release,
# with the archive hashes taken from the release's .sha256 files:
#
#   scripts/formula.sh 0.1.0 > ../homebrew-tap/Formula/speak.rb
set -eu
VERSION=$1
REPO=peetzweg/speak
BASE=https://github.com/$REPO/releases/download/v$VERSION

sha() {
  gh release download "v$VERSION" -R "$REPO" -p "speak-$VERSION-$1.tar.gz.sha256" -O - | cut -d' ' -f1
}
MAC_ARM=$(sha aarch64-apple-darwin)
LINUX_X86=$(sha x86_64-unknown-linux-gnu)
LINUX_ARM=$(sha aarch64-unknown-linux-gnu)

cat <<EOF
class Speak < Formula
  desc "Like say(1), with the best open text-to-speech model, running locally"
  homepage "https://github.com/$REPO"
  version "$VERSION"
  # speak is MIT OR Apache-2.0; the bundled espeak-ng is GPL-3.0-or-later.
  license all_of: [any_of: ["MIT", "Apache-2.0"], "GPL-3.0-or-later"]

  on_macos do
    depends_on arch: :arm64

    url "$BASE/speak-$VERSION-aarch64-apple-darwin.tar.gz"
    sha256 "$MAC_ARM"
  end

  on_linux do
    on_intel do
      url "$BASE/speak-$VERSION-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$LINUX_X86"
    end
    on_arm do
      url "$BASE/speak-$VERSION-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$LINUX_ARM"
    end
  end

  conflicts_with "espeak-ng", because: "both install a \`speak\` executable"

  def install
    bin.install "bin/speak"
    # speak finds its espeak-ng in <prefix>/libexec/speak, next to its own bin/.
    libexec.install "libexec/speak"
    prefix.install "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "licenses"
  end

  def caveats
    <<~EOS
      The first run downloads the Kokoro model (354 MB) into ~/.local/share/speak.
      For agents: \`speak --background "Build finished"\`, and \`speak stop\`.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/speak --version")
    # The bundled espeak-ng reads the Spanish voice; no model is needed for --analyze.
    ENV["SPEAK_NO_DOWNLOAD"] = "1"
    assert_match "ˈola", shell_output("#{bin}/speak --analyze -v ef_dora hola")
  end
end
EOF
