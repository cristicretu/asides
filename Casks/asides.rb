# This repository is also a Homebrew tap. scripts/release.sh updates the version and checksum;
# commit the change after the release is published on GitHub.
cask "asides" do
  version "0.2.0"
  sha256 "7d0ed9b9c4e50ec00e9721324d8864b227f22b0660db986e3266d456372990f9"

  url "https://github.com/mihaicristianfarcas/asides/releases/download/v#{version}/Asides-#{version}.zip"
  name "Asides"
  desc "Records spoken bug reports and transcribes them locally into Markdown notes"
  homepage "https://mihaicristianfarcas.github.io/asides/"

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
