# This repository is also a Homebrew tap. scripts/release.sh updates the version and checksum;
# commit the change after the release is published on GitHub.
cask "asides" do
  version "0.6.2"
  sha256 "9fc05e09c9dad28e0e7d35ee44f3880ad32dbad9fe809b37a383b0b278e69c8e"

  url "https://github.com/mihaicristianfarcas/asides/releases/download/v#{version}/Asides-#{version}.zip"
  name "Asides"
  desc "Records spoken bug reports and transcribes them locally into Markdown notes"
  homepage "https://asides.mihaicristian.dev/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "Asides.app"
  binary "#{appdir}/Asides.app/Contents/Helpers/asides"

  uninstall quit: "io.github.mihaicristianfarcas.asides"

  zap trash: "~/Library/Application Support/Asides"
end
