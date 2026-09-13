const repository = 'https://github.com/godosou/outsie';
const tag = 'v0.7.0';

export const macRelease = {
  version: '0.7.0',
  highlights: {
    zh: '新版：手机钥匙。走近按回车就进，走远自动锁屏；手机上点一下，Mac 切到那个 App 替你按键。',
    en: 'New: Phone Key. Walk up and press Enter to get in, walk away and the Mac locks; tap on the phone and the Mac switches to that app and presses the keys.',
  },
  pageUrl: `${repository}/releases/tag/${tag}`,
  downloadUrl: `${repository}/releases/download/${tag}/Outsie-0.7.0-mac-arm64.dmg`,
  androidDownloadUrl: `${repository}/releases/download/${tag}/Outsie-0.7.0-android.apk`,
  checksumUrl: `${repository}/releases/download/${tag}/SHA256SUMS.txt`,
};
