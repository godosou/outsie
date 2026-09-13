const repository = 'https://github.com/godosou/outsie';
const tag = 'v0.8.0';

export const macRelease = {
  version: '0.8.0',
  highlights: {
    zh: '这一版：手机钥匙、快捷控制、会议检测。走近按回车就进，走远自动锁屏；手机上点一下，Mac 切到那个 App 替你按键；开会时到点的休息，等会议结束再来。',
    en: 'This release: Phone Key, quick controls and meeting detection. Walk up and press Enter to get in, walk away and the Mac locks; tap on the phone and the Mac switches to that app and presses the keys; a break that comes due in a meeting waits for the call to end.',
  },
  pageUrl: `${repository}/releases/tag/${tag}`,
  downloadUrl: `${repository}/releases/download/${tag}/Outsie-0.8.0-mac-arm64.dmg`,
  androidDownloadUrl: `${repository}/releases/download/${tag}/Outsie-0.8.0-android.apk`,
  checksumUrl: `${repository}/releases/download/${tag}/SHA256SUMS.txt`,
};
