# This repository is also a Homebrew tap. scripts/release.sh updates the version and checksum;
# commit the change after the release is published on GitHub.
cask "explainit" do
  version "0.1.0"
  sha256 "3acb415761b3eec3c62d1e7623357584363610ebce807abe23828b66724a8790"

  url "https://github.com/mihaicristianfarcas/explainit/releases/download/v#{version}/ExplainIt-#{version}.zip"
  name "ExplainIt"
  desc "Records spoken bug reports and transcribes them locally into Markdown notes"
  homepage "https://mihaicristianfarcas.github.io/explainit/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "ExplainIt.app"
  binary "#{appdir}/ExplainIt.app/Contents/Helpers/explainit"

  uninstall quit: "io.github.mihaicristianfarcas.explainit"

  zap trash: "~/Library/Application Support/ExplainIt"
end
