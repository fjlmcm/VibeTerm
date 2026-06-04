# VibeTerm Homebrew cask —— 唯一真相模板。
# update-homebrew.yml 在每次稳定版 Release 完成后,把 __VERSION__ / __SHA256__ 替换为实际值,
# 推送到独立 tap 仓 `fjlmcm/homebrew-vibeterm`(brew tap 要求仓名带 homebrew- 前缀)。
#
# 安装:
#   brew tap fjlmcm/vibeterm
#   brew install --cask vibeterm
cask "vibeterm" do
  version "__VERSION__"
  sha256 "__SHA256__"

  url "https://github.com/fjlmcm/VibeTerm/releases/download/v#{version}/VibeTerm_#{version}_universal.dmg",
      verified: "github.com/fjlmcm/VibeTerm/"
  name "VibeTerm"
  desc "CJK-first, local-first terminal manager for multi-agent workflows"
  homepage "https://github.com/fjlmcm/VibeTerm"

  # 应用内自更新由 tauri-plugin-updater 处理;Homebrew 不再重复管理 livecheck。
  auto_updates true
  depends_on macos: ">= :big_sur"

  app "VibeTerm.app"

  zap trash: [
    "~/Library/Application Support/com.vibeterm.desktop",
    "~/Library/Application Support/VibeTerm",
    "~/Library/Caches/com.vibeterm.desktop",
    "~/Library/Preferences/com.vibeterm.desktop.plist",
    "~/Library/Saved Application State/com.vibeterm.desktop.savedState",
  ]
end
