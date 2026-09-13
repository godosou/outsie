'use client';

import { useEffect, useState, useSyncExternalStore } from 'react';
import Image from 'next/image';
import { publicAsset } from '@/lib/public-asset';
import { macRelease } from '@/lib/release';
import { PhoneControls } from './phone-controls';
import { HeroExperience } from './hero-experience';
import { BreakDemo, StretchDemo, PhoneKeyDemo } from './product-demos';
import {
  ArrowRight,
  Check,
  Download,
  Headphones,
  KeyRound,
  LockKeyhole,
  Monitor,
  Pause,
  ShieldCheck,
  Smartphone,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';

type Language = 'zh' | 'en';

/** The one thing this site stores: the language the reader picked. */
const LANG_KEY = 'outsie.lang';

/**
 * Language lives in a small external store rather than state seeded by an
 * effect. The page is prerendered in Chinese, so the server snapshot has to
 * stay 'zh' while the client resolves the real preference during hydration —
 * seeding it with setState inside an effect would cascade renders instead.
 *
 * A first visit follows the browser's own language list. An explicit choice is
 * remembered and never overridden afterwards. There is no IP lookup: this site
 * is served as a static export, so reading location would mean handing a
 * visitor's address to a third party, and the browser's declared language is
 * the better signal anyway.
 */
let chosenLang: Language | null = null;
let langListeners: Array<() => void> = [];

function readLang(): Language {
  if (chosenLang) return chosenLang;
  try {
    const stored = window.localStorage.getItem(LANG_KEY);
    if (stored === 'zh' || stored === 'en') return stored;
  } catch {
    // Private browsing: fall through to the browser's language list.
  }
  const list = navigator.languages?.length
    ? navigator.languages
    : [navigator.language ?? ''];
  return list.some((l) => l.toLowerCase().startsWith('zh')) ? 'zh' : 'en';
}

function readServerLang(): Language {
  return 'zh';
}

function subscribeLang(onChange: () => void) {
  langListeners = [...langListeners, onChange];
  return () => {
    langListeners = langListeners.filter((listener) => listener !== onChange);
  };
}

function chooseLang(next: Language) {
  chosenLang = next;
  try {
    window.localStorage.setItem(LANG_KEY, next);
  } catch {
    // Private browsing: the choice still holds for this visit.
  }
  langListeners.forEach((listener) => listener());
}

export default function Home() {
  const lang = useSyncExternalStore(subscribeLang, readLang, readServerLang);
  const [demo, setDemo] = useState({ seconds: 20, running: false });
  const { seconds, running } = demo;
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  useEffect(() => {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
  }, [lang]);
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(
      () =>
        setDemo((current) => {
          const remaining = Math.max(0, current.seconds - 1);
          return { seconds: remaining, running: remaining > 0 };
        }),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [running]);
  function startDemo() {
    setDemo({ seconds: seconds === 0 ? 20 : seconds, running: true });
    document.getElementById('break-demo')?.scrollIntoView({
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches
        ? 'instant'
        : 'smooth',
      block: 'center',
    });
  }

  return (
    <div className="site" lang={lang === 'zh' ? 'zh-CN' : 'en'}>
      <a className="skip-link" href="#main">
        {t('跳转到正文', 'Skip to content')}
      </a>
      <header className="site-header shell">
        <a className="wordmark" href="#main" aria-label="Outsie">
          <Image
            src={publicAsset('/favicon.svg')}
            alt=""
            width={40}
            height={40}
            unoptimized
          />
          outsie<span>.</span>
        </a>
        <nav aria-label={t('主导航', 'Main navigation')}>
          <a href="#rhythm">{t('人类休息计划', 'Breaks')}</a>
          <a href="#stretch">{t('拉伸跟练', 'Stretching')}</a>
          <a href="#phone-key">{t('手机钥匙', 'Phone Key')}</a>
          <a href="#phone-controls">{t('手机工作台', 'Phone Controls')}</a>
          <a href="#download">{t('下载 Mac 版', 'Download for Mac')}</a>
        </nav>
        <div className="language-switch" aria-label={t('语言', 'Language')}>
          <Button
            variant="ghost"
            aria-pressed={lang === 'zh'}
            onClick={() => chooseLang('zh')}
          >
            中
          </Button>
          <span aria-hidden="true">/</span>
          <Button
            variant="ghost"
            aria-pressed={lang === 'en'}
            onClick={() => chooseLang('en')}
          >
            EN
          </Button>
        </div>
      </header>
      <main id="main">
        <section className="hero shell" aria-labelledby="hero-title">
          <div className="hero-copy">
            <p className="eyebrow">
              <span className="live-dot" />
              {t('为和 AI 一起工作的你', 'FOR THE HUMAN BEHIND THE PROMPT')}
            </p>
            <h1 id="hero-title">
              {t('AI 时代，', 'In the age of AI,')}
              <br />
              <em>{t('先照顾好自己。', 'put yourself first.')}</em>
            </h1>
            <p className="hero-description">
              {t(
                '按时休息、起身拉伸；用手机和耳机，少敲点键盘。',
                'Scheduled breaks and guided stretches. A phone and a headset, for less typing.',
              )}
            </p>
            <div className="hero-actions">
              <a className="primary-cta" href="#download">
                {t('下载 Mac 版', 'Download for Mac')}
                <Download size={18} aria-hidden="true" />
              </a>
              <Button
                className="hero-rest-cta"
                variant="outline"
                onClick={startDemo}
              >
                {t('先歇 20 秒', 'Take a 20-second break')}
                <Pause size={17} aria-hidden="true" />
              </Button>
            </div>
            <p className="platform-note hero-device-note">
              <Monitor size={16} aria-hidden="true" /> Mac
              <span aria-hidden="true">＋</span>
              <Smartphone size={16} aria-hidden="true" /> {t('手机', 'Phone')}
              <span aria-hidden="true">＋</span>
              <Headphones size={16} aria-hidden="true" /> {t('耳机', 'Headset')}
            </p>
            <a className="hero-download-note" href="#download">
              {t('现已提供', 'Available now:')} v{macRelease.version} · macOS
              14+ · Apple Silicon
            </a>
          </div>
          <HeroExperience
            lang={lang}
            seconds={seconds}
            running={running}
            onToggleBreak={() =>
              seconds === 0
                ? startDemo()
                : setDemo({ seconds, running: !running })
            }
          />
        </section>
        <section className="manifesto shell">
          <h2>
            {t('“再改一个。”', '“Just one more fix.”')}
            <br />
            <span>{t('你也说过，对吧。', 'Sound familiar?')}</span>
          </h2>
        </section>
        <section className="rhythm-section" id="rhythm">
          <div className="shell">
            <div className="section-heading">
              <h2>{t('该歇就歇，放心离开。', 'Make room to step away.')}</h2>
            </div>
            <div className="rhythm-panel">
              <div className="rhythm-copy">
                <span className="feature-status">
                  <span className="live-dot" />
                  {t('定时休息', 'Scheduled breaks')}
                </span>
                <h3>{t('“马上”到此为止。', '“In a minute” has a limit.')}</h3>
                <p>
                  {t(
                    '休息时间一到，Outsie 就会遮住所有显示器，让你停下来歇一会儿。每次可以推迟一次，再提醒时就得把这次休息完成。',
                    'When a break begins, Outsie covers every connected display. You get one postponement. When that time is up, your break gets its turn.',
                  )}
                </p>
              </div>
              <div
                className="flow-diagram"
                aria-label={t('这次，先把休息安排上。', 'Your break gets its turn.')}
              >
                <Pause size={48} strokeWidth={1.25} aria-hidden="true" />
                <code>human.pause()</code>
                <span>
                  {t('这次，先把休息安排上。', 'Your break gets its turn.')}
                </span>
              </div>
            </div>
            <BreakDemo lang={lang} />
            <p className="section-note">
              {t(
                '开会时不打扰：Zoom、Teams、飞书、腾讯会议通话中，到点的休息等会议结束再来，会议时间单独记录。也可开启闲置锁屏：系统键鼠闲置 30 秒自动锁屏。',
                'Meetings come first: on a call in Zoom, Teams, Feishu or Tencent Meeting, a break that comes due waits for the call to end, and meeting time is recorded separately. You can also enable idle locking, which locks the Mac after 30 seconds without keyboard or mouse activity.',
              )}
            </p>
          </div>
        </section>
        <section className="body-section" id="stretch">
          <div className="shell">
            <div className="section-heading">
              <h2>
                {t('AI 没有肩颈。', 'AI has no shoulders.')}
                <br />
                {t('你有，别僵着。', 'Yours need a break.')}
              </h2>
            </div>
            <div className="stretch-detail">
              <div className="stretch-facts">
                <span className="stretch-number">8</span>
                <h3>{t('个离线 3D 拉伸动作', 'offline 3D stretches')}</h3>
                <p>
                  {t(
                    '每 30 秒换一个动作，从肩颈到上背，再到手腕和身体两侧。',
                    'A new movement every 30 seconds, from your neck and upper back to your wrists and sides.',
                  )}
                </p>
                <div className="stretch-local">
                  <Check size={17} aria-hidden="true" />
                  {t(
                    '无需联网 · 动作要领 · 跟练进度',
                    'Works offline · Movement cues · Progress',
                  )}
                </div>
              </div>
              <ol
                className="movement-list"
                aria-label={t('拉伸动作', 'Stretching movements')}
              >
                {[
                  t('下巴微收', 'Chin tuck'),
                  t('颈部侧向拉伸', 'Side neck stretch'),
                  t('肩部向后环绕', 'Backward shoulder rolls'),
                  t('上斜方肌拉伸', 'Upper trapezius stretch'),
                  t('胸肩打开', 'Chest opener'),
                  t('上背旋转', 'Upper back rotation'),
                  t('手腕与前臂拉伸', 'Wrist & forearm stretch'),
                  t('站立侧弯', 'Standing side bend'),
                ].map((movement, index) => (
                  <li key={movement}>
                    <span aria-hidden="true">
                      {String(index + 1).padStart(2, '0')}
                    </span>
                    {movement}
                  </li>
                ))}
              </ol>
            </div>
            <StretchDemo lang={lang} />
            <ul className="stretch-specs">
              <li>
                {t(
                  '呼吸圆圈：吸气 4 秒，停留 4 秒，呼气 6 秒。',
                  'Breathing guide: in for 4 seconds, hold for 4, out for 6.',
                )}
              </li>
              <li>
                {t(
                  '选一套现成的休息计划，或自定工作间隔与休息时长。',
                  'Choose a preset break plan, or set your own work interval and break length.',
                )}
              </li>
              <li>
                {t(
                  '按小时看工作与休息，35 天记录，可导出近 7 天 CSV；记录留在本机。',
                  'See work and rest by the hour, review 35 days, export the last 7 as CSV. Records stay on your device.',
                )}
              </li>
              <li>
                {t(
                  '锁屏或睡眠计入休息；休息足够长，就重排下一轮。',
                  'Time locked or asleep counts as rest; a long enough rest resets the next cycle.',
                )}
              </li>
            </ul>
          </div>
        </section>
        <section className="phone-section shell" id="phone-key">
          <div className="phone-copy">
            <h2>
              {t('你带走手机。', 'Take your phone.')}
              <br />
              <em>{t('我们照看屏幕。', 'Leave the screen to us.')}</em>
            </h2>
          </div>
          <div className="phone-plan">
            <div className="phone-plan-header">
              <KeyRound size={21} aria-hidden="true" />
              <span>PHONE KEY</span>
              <span>{t('三步设置', 'THREE SIMPLE STEPS')}</span>
            </div>
            <ol>
              {[
                [
                  t('扫码配对手机', 'Scan to pair'),
                  t(
                    '用手机扫描 Mac 上的一次性二维码。',
                    'Scan the one-time QR code on your Mac.',
                  ),
                ],
                [
                  t('校准你的距离', 'Calibrate your space'),
                  t(
                    '在常用位置采集靠近与离开的信号。',
                    'Learn the nearby and away signals at your desk.',
                  ),
                ],
                [
                  t('走开，再回来', 'Leave. Return.'),
                  t(
                    '离开锁屏，返回后验证手机钥匙。',
                    'Lock when you leave. Verify your key when you return.',
                  ),
                ],
              ].map(([title, body], i) => (
                <li key={i}>
                  <span className="step-number">0{i + 1}</span>
                  <div>
                    <h3>{title}</h3>
                    <p>{body}</p>
                  </div>
                </li>
              ))}
            </ol>
            <p className="phone-plan-note">
              <Smartphone size={17} aria-hidden="true" />
              {t(
                '手机与 Mac 配对，钥匙随身带',
                'Pair your phone and Mac. Keep your key with you.',
              )}
            </p>
          </div>
          <PhoneKeyDemo lang={lang} />
          <div className="trust-row">
            <article>
              <ShieldCheck aria-hidden="true" />
              <div>
                <h3>{t('密码仍由你掌握', 'Your password stays yours')}</h3>
                <p>
                  {t(
                    '手机钥匙按不保存、不模拟输入 Mac 密码的方式设计。',
                    'Phone Key is designed without storing or typing your Mac password.',
                  )}
                </p>
              </div>
            </article>
            <article>
              <Monitor aria-hidden="true" />
              <div>
                <h3>
                  {t('配对发生在你的设备之间', 'Between your own devices')}
                </h3>
                <p>
                  {t(
                    '距离信号与解锁验证按本地处理设计。',
                    'Proximity signals and unlock verification are designed to stay local.',
                  )}
                </p>
              </div>
            </article>
            <article>
              <LockKeyhole aria-hidden="true" />
              <div>
                <h3>{t('系统验证始终保留', 'Keep system authentication')}</h3>
                <p>
                  {t(
                    '重启后的首次登录仍需系统验证；手机钥匙不可用时使用密码。',
                    'Use system authentication after a restart, and your password whenever Phone Key is unavailable.',
                  )}
                </p>
              </div>
            </article>
          </div>
        </section>
        <PhoneControls lang={lang} />
        <section
          className="download-section shell"
          id="download"
          aria-labelledby="download-title"
        >
          <div className="download-copy">
            <h2 id="download-title">
              {t('把休息，留进日常。', 'Make room for a daily pause.')}
            </h2>
            <div className="download-actions">
              <a className="primary-cta" href={macRelease.downloadUrl}>
                <Download size={18} aria-hidden="true" />
                {t('下载 Mac 安装包', 'Download for Mac')}
              </a>
              <a className="text-link" href={macRelease.androidDownloadUrl}>
                <Smartphone size={16} aria-hidden="true" />
                {t('Android 版（手机钥匙）', 'Android (Phone Key)')}
              </a>
              <a className="text-link" href={macRelease.pageUrl}>
                {t('查看发布说明', 'Release notes')}
                <ArrowRight size={16} aria-hidden="true" />
              </a>
            </div>
            <p className="download-meta">
              v{macRelease.version} · {t('预览版', 'Preview')} · Apple Silicon ·
              macOS 14+ · Android 12+
            </p>
            <p className="download-scope">{macRelease.highlights[lang]}</p>
          </div>
          <div className="download-guide">
            <h3>
              {t(
                '三步开始，给自己一点空白。',
                'Three steps to a little breathing room.',
              )}
            </h3>
            <ol>
              <li>
                {t(
                  '打开 DMG，将 Outsie 拖进“应用程序”。',
                  'Open the DMG and drag Outsie into Applications.',
                )}
              </li>
              <li>
                {t(
                  '打开 Outsie，按自己的节奏设置休息时间。',
                  'Open Outsie and choose your break schedule.',
                )}
              </li>
              <li>
                {t(
                  '要用手机钥匙：手机装上 Android 版，在 Mac 的「手机控制」里点「配一部新手机」。',
                  'For Phone Key: install the Android app, then choose “Pair a new phone” under Phone Controls on the Mac.',
                )}
              </li>
            </ol>
            <p>
              {t(
                '当前版本尚未经过 Apple 公证。首次打开若被系统拦截，请先确认下载来源，再参考 Apple 的打开指引。',
                'This version is not notarized by Apple. If macOS blocks the first launch, confirm the download source and follow Apple’s opening instructions.',
              )}
            </p>
            <div className="download-help-links">
              <a href="https://support.apple.com/102445">
                {t('Apple 打开指引', 'Apple’s opening guide')}
              </a>
              <a href={macRelease.checksumUrl}>
                {t('SHA-256 校验文件', 'SHA-256 checksums')}
              </a>
            </div>
          </div>
        </section>
        <section className="faq-section shell">
          <div>
            <h2>{t('你可能想问。', 'A few human questions.')}</h2>
          </div>
          <Accordion className="faq-list" multiple>
            {[
              [
                t('它会真的让我停下来吗？', 'Will it actually make me stop?'),
                t(
                  'Mac 应用会在休息时覆盖所有显示器，并限制常见的应用切换和退出操作。每次休息允许延期一次。系统级强制退出等操作仍由 macOS 保留；主页上的计时器只是演示。',
                  'The Mac app covers every display during a break and restricts common app-switching and quit actions. Each break allows one postponement. macOS still controls system-level actions such as force quit. The timer on this page is only a demo.',
                ),
              ],
              [
                t(
                  '手机靠近，会不会把休息也取消了？',
                  'Will returning with my phone skip a break?',
                ),
                t(
                  '不会。手机钥匙负责恢复系统访问，休息倒计时独立进行。回来时如果休息还没结束，就把剩余时间留给自己。',
                  'No. Phone Key restores system access while the break timer runs independently. If you return before the break ends, the remaining time is still yours to rest.',
                ),
              ],
              [
                t('为什么还要用手机和耳机？', 'Why a phone and headset?'),
                t(
                  '出发点是减少手部负担。让手机承担少量选择和控制，语音承担长段输入，帮助你离开键盘鼠标、换个姿势继续和 AI 协作。',
                  'The goal is to give your hands less repetitive work: a few choices on the phone, longer input by voice. That lets you change position and collaborate with AI away from the keyboard and mouse.',
                ),
              ],
              [
                t('是不是只有程序员能用？', 'Is Outsie only for developers?'),
                t(
                  '任何长时间坐在屏幕前的人都可以使用休息功能。AI Coding 是我们的出发点：总有下一条 prompt，也该有下一次休息。',
                  'Anyone who spends long stretches at a screen can use the break features. AI coding is where our story starts: there’s always another prompt. There should be another break, too.',
                ),
              ],
            ].map(([question, answer], i) => (
              <AccordionItem key={i} value={i}>
                <AccordionTrigger>{question}</AccordionTrigger>
                <AccordionContent>
                  <p>{answer}</p>
                </AccordionContent>
              </AccordionItem>
            ))}
          </Accordion>
        </section>
        <section className="closing-section shell">
          <h2>
            {t('下一条 prompt 之前，', 'Before the next prompt,')}
            <br />
            <em>{t('先给自己充个电。', 'recharge the human.')}</em>
          </h2>
          <a className="primary-cta" href="#download">
            {t('下载 Mac 版', 'Download for Mac')}
            <Download size={18} aria-hidden="true" />
          </a>
          <span>macOS 14+ · Apple Silicon · v{macRelease.version}</span>
        </section>
        <footer className="site-footer shell">
          <a className="wordmark" href="#main">
            outsie<span>.</span>
          </a>
          <p>Let AI run. Stay human.</p>
          <p className="footer-notice">
            {t(
              '3D 人体来自 CC0 的 MakeHuman 素材；红色仅为拉伸区域示意，并非精确肌肉解剖。动作以舒适为准，如有疼痛或眩晕请立即停止。',
              'The 3D figure is built from CC0 MakeHuman assets. Red shading only indicates the stretch region and is not precise anatomy. Move within a comfortable range, and stop immediately if you feel pain or dizziness.',
            )}
          </p>
          <p className="footer-notice">
            {t(
              '本站不使用 cookie，没有统计、广告或第三方服务。',
              'This site sets no cookies and uses no analytics, ads or third-party services.',
            )}
          </p>
          <span>© {new Date().getFullYear()} Outsie</span>
        </footer>
      </main>
    </div>
  );
}
