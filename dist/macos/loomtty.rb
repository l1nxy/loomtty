# Homebrew cask for loomtty (macOS, Apple Silicon).
#
# Installs loomtty.app to /Applications and symlinks the `loomtty` and
# `loomtty-server` CLI binaries (inside the bundle) onto the PATH, so the same
# install serves the GUI (Finder/Launchpad) and the CLI / remote-attach use.
#
# Distribute via a tap, e.g. a `l1nxy/homebrew-loomtty` repo holding
# `Casks/loomtty.rb`, then:  brew install --cask l1nxy/loomtty/loomtty
# (For a quick local try: `brew install --cask ./dist/macos/loomtty.rb`.)
cask "loomtty" do
  version "0.1.0"
  sha256 :no_check # TODO: pin to the released .dmg sha256 per release

  url "https://github.com/l1nxy/loomtty/releases/download/v#{version}/loomtty-#{version}-macos-arm64.dmg",
      verified: "github.com/l1nxy/loomtty/"
  name "loomtty"
  desc "GPU-accelerated terminal multiplexer"
  homepage "https://github.com/l1nxy/loomtty"

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "loomtty.app"
  binary "#{appdir}/loomtty.app/Contents/MacOS/loomtty"
  binary "#{appdir}/loomtty.app/Contents/MacOS/loomtty-server"

  zap trash: [
    "~/.config/loom",
    "~/Library/Application Support/loom",
    "~/Library/Caches/loom",
  ]
end
