# This repository is also a Homebrew tap. scripts/release.sh updates the version and checksum;
# commit the change after the release is published on GitHub.
cask "asides" do
  version "0.4.0"
  sha256 "89ddaba83c23c7040775f0cb3253066cf7af7df2e6cd92d81848bba0c835ad75"

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
