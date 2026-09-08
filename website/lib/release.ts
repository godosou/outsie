const repository = 'https://github.com/godosou/outsie';
const tag = 'v0.6.3';

export const macRelease = {
  version: '0.6.3',
  highlights: {
    zh: '新版：更大的 3D 人物，右侧集中显示动作、倒计时与操作；短休息新增 6 条护眼知识。',
    en: 'New: a larger 3D guide with movement cues, countdown and controls on the right, plus six eye-care tips for short breaks.',
  },
  pageUrl: `${repository}/releases/tag/${tag}`,
  downloadUrl: `${repository}/releases/download/${tag}/Repose-0.6.3-mac-arm64.dmg`,
  checksumUrl: `${repository}/releases/download/${tag}/SHA256SUMS.txt`,
};
