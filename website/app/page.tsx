'use client';

import { useEffect, useState } from 'react';
import Image from 'next/image';
import { publicAsset } from '@/lib/public-asset';
import { macRelease } from '@/lib/release';
import { PhoneControls } from './phone-controls';
import { HeroExperience } from './hero-experience';
import { BreakDemo, StretchDemo, PhoneKeyDemo } from './product-demos';
import {
  ArrowRight,
  BarChart3,
  Check,
  Download,
  Eye,
  Headphones,
  KeyRound,
  LockKeyhole,
  Monitor,
  Pause,
  ShieldCheck,
  SlidersHorizontal,
  Smartphone,
  Sprout,
  Video,
  Wind,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion';

type Language = 'zh' | 'en';
export default function Home() {
  const [lang, setLang] = useState<Language>('zh');
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
            onClick={() => setLang('zh')}
          >
            中
          </Button>
          <span aria-hidden="true">/</span>
          <Button
            variant="ghost"
            aria-pressed={lang === 'en'}
            onClick={() => setLang('en')}
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
            <p className="hero-promise">
              {t('别让 AI 把你榨干。', 'Don’t let AI drain you.')}
            </p>
            <p className="hero-description">
              {t(
                '按时休息、起身拉伸；用手机控制电脑，用耳机语音说出想法。Outsie 帮你少坐一会儿、少敲点键盘，和 AI 一起工作，也照顾好自己。',
                'Pause and stretch on time. Control your Mac from your phone and speak your ideas through a headset. Less sitting, less typing, and more room for yourself while working with AI.',
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
        <div className="human-strip">
          <span>LESS SITTING.</span>
          <span aria-hidden="true">✳</span>
          <strong>MORE YOU.</strong>
          <span aria-hidden="true">✳</span>
          <span>LESS TYPING.</span>
        </div>
        <section className="manifesto shell">
          <p className="eyebrow">THE INFINITE LOOP</p>
          <div>
            <h2>
              {t('“再改一个。”', '“Just one more fix.”')}
              <br />
              <span>{t('你也说过，对吧。', 'Sound familiar?')}</span>
            </h2>
            <p>
              {t(
                '等它生成，看完结果，再追问一句。总觉得马上就好，一抬头，几个小时过去了。Outsie 到点就来催你，把那句“等会儿再休息”变成一次真正的休息。',
                'Wait for the output. Read it. Ask again. Each step takes a moment. Together, they can take your afternoon. Give the loop a break that actually happens.',
              )}
            </p>
          </div>
          <span className="loop-symbol" aria-hidden="true">
            ↻
          </span>
        </section>
        <section className="rhythm-section" id="rhythm">
          <div className="shell">
            <p className="eyebrow">A WAY OUT, BUILT IN</p>
            <div className="section-heading">
              <h2>{t('该歇就歇，放心离开。', 'Make room to step away.')}</h2>
              <p>
                {t(
                  '到点休息，离开锁屏，回来继续。把每次起身需要操心的小事，交给 Outsie。',
                  'Pause, step away, come back. Outsie takes care of the little things around each break.',
                )}
              </p>
            </div>
            <Tabs defaultValue="break" className="rhythm-tabs">
              <TabsList
                className="rhythm-tabs-list"
                aria-label={t('体验步骤', 'Experience steps')}
              >
                <TabsTrigger value="break">
                  {t('01 / 到点休息', '01 / Take a break')}
                </TabsTrigger>
                <TabsTrigger value="away">
                  {t('02 / 放心离开', '02 / Step away')}
                </TabsTrigger>
                <TabsTrigger value="back">
                  {t('03 / 回来继续', '03 / Come back')}
                </TabsTrigger>
              </TabsList>
              {[
                {
                  id: 'break',
                  Icon: Pause,
                  title: t('“马上”到此为止。', '“In a minute” has a limit.'),
                  body: t(
                    '休息时间一到，Outsie 就会遮住所有显示器，让你停下来歇一会儿。每次可以推迟一次，再提醒时就得把这次休息完成。正在开会？到点的休息会等会议结束再来。',
                    'When a break begins, Outsie covers every connected display. You get one postponement. When that time is up, your break gets its turn. In a meeting? The break waits for the call to end.',
                  ),
                  code: 'human.pause()',
                  detail: t(
                    '这次，先把休息安排上。',
                    'Your break gets its turn.',
                  ),
                },
                {
                  id: 'away',
                  Icon: LockKeyhole,
                  title: t(
                    '你离开，电脑自动锁屏。',
                    'You step away. Your Mac locks.',
                  ),
                  body: t(
                    '带着配对手机离开，Mac 自动锁屏。也可以开启闲置锁屏，让屏幕在一段时间没有键鼠操作后自动锁定。',
                    'Walk away with your paired phone and your Mac locks automatically. You can also enable idle locking after a period without keyboard or mouse activity.',
                  ),
                  code: 'desk.lock()',
                  detail: t('离席信号 → Mac 锁屏', 'Away signal → Mac locked'),
                },
                {
                  id: 'back',
                  Icon: Smartphone,
                  title: t(
                    '手机带在身边，回来就能继续。',
                    'Bring your phone. Bring yourself.',
                  ),
                  body: t(
                    '完成配对和距离校准，手机就是你的随身钥匙。离开再回来，验证手机钥匙后恢复已登录的 Mac 会话。',
                    'Pair your phone and calibrate the distance. When you leave and return, Phone Key verifies your phone and restores your existing Mac session.',
                  ),
                  code: 'welcome.back()',
                  detail: t(
                    '离开再返回 → 验证手机钥匙',
                    'Leave, return → Verify Phone Key',
                  ),
                },
              ].map(({ id, Icon, title, body, code, detail }) => (
                <TabsContent key={id} value={id} className="rhythm-panel">
                  <div className="rhythm-copy">
                    <span className="feature-status">
                      <span className="live-dot" />
                      {id === 'break'
                        ? t('定时休息', 'Scheduled breaks')
                        : t('手机钥匙', 'Phone Key')}
                    </span>
                    <h3>{title}</h3>
                    <p>{body}</p>
                  </div>
                  <div className="flow-diagram" aria-label={detail}>
                    <Icon size={48} strokeWidth={1.25} aria-hidden="true" />
                    <code>{code}</code>
                    <span>{detail}</span>
                  </div>
                </TabsContent>
              ))}
            </Tabs>
            <BreakDemo lang={lang} />
            <div className="feature-row">
              {[
                {
                  Icon: Eye,
                  title: t('让眼睛歇一会儿', 'Room for your eyes'),
                  body: t(
                    '每次短休息一条护眼知识，读完就看远处。',
                    'One eye-care tip per short break. Read it, then look into the distance.',
                  ),
                },
                {
                  Icon: Sprout,
                  title: t('跟着做，松松肩颈', 'Remember your shoulders?'),
                  body: t(
                    '放大的离线 3D 人物在左，动作说明与倒计时在右。',
                    'A larger offline 3D guide on the left, with cues and countdown on the right.',
                  ),
                },
                {
                  Icon: ShieldCheck,
                  title: t('温和，但来真的', 'Friendly. Firm.'),
                  body: t(
                    '多显示器覆盖，每次休息只给一次延期。',
                    'Every display covered. One postponement per break.',
                  ),
                },
              ].map(({ Icon, title, body }) => (
                <article key={title}>
                  <Icon aria-hidden="true" />
                  <h3>{title}</h3>
                  <p>{body}</p>
                </article>
              ))}
            </div>
          </div>
        </section>
        <section className="body-section" id="stretch">
          <div className="shell">
            <p className="eyebrow">YOUR AI HAS NO SHOULDERS. YOU DO.</p>
            <div className="section-heading">
              <h2>
                {t('AI 没有肩颈。', 'AI has no shoulders.')}
                <br />
                {t('你有，别僵着。', 'Yours need a break.')}
              </h2>
              <p>
                {t(
                  '休息时不知道做什么？跟着 3D 示范动一动。看看动作要领，松松肩颈、伸伸手腕，按自己舒服的幅度来。',
                  'Not sure what to do on a break? Follow the 3D guide, read the movement cues, and give your shoulders and wrists a little attention. Move within a comfortable range.',
                )}
              </p>
            </div>
            <div className="stretch-detail">
              <div className="stretch-facts">
                <span className="stretch-number">8</span>
                <h3>{t('个离线 3D 拉伸动作', 'offline 3D stretches')}</h3>
                <p>
                  {t(
                    '每 30 秒换一个动作，从肩颈到上背，再到手腕和身体两侧。可以自动跟练，也可以手动切换到上一个或下一个动作。',
                    'A new movement every 30 seconds, from your neck and upper back to your wrists and sides. Follow the sequence or switch between movements yourself.',
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
            <div className="everyday-features">
              {[
                {
                  Icon: Wind,
                  title: t('喝口水，慢慢呼吸', 'Sip. Breathe. Reset.'),
                  body: t(
                    '休息卡片会建议你喝水、远眺。也可以跟着呼吸圆圈做一轮：吸气 4 秒，停留 4 秒，呼气 6 秒。',
                    'Break cards suggest drinking water or looking into the distance. A breathing guide offers a gentle rhythm: in for 4 seconds, hold for 4, out for 6.',
                  ),
                },
                {
                  Icon: SlidersHorizontal,
                  title: t('按你的节奏来', 'Find your rhythm'),
                  body: t(
                    '选一套现成的休息计划，或自己调整工作间隔、短休息时长，以及隔几次安排一次长休息。',
                    'Choose a preset or set your own work interval, short-break duration, and how often a longer break comes around.',
                  ),
                },
                {
                  Icon: BarChart3,
                  title: t(
                    '今天，真的歇过了吗？',
                    'Did you actually take a break?',
                  ),
                  body: t(
                    '按小时看看工作和休息分布，翻看最近 35 天的记录，也能导出近 7 天的 CSV。记录保存在本机。',
                    'See work and rest by the hour, review the last 35 days, or export the past 7 days as a CSV. Your records stay on your device.',
                  ),
                },
                {
                  Icon: Monitor,
                  title: t('已经歇过，就算数', 'Time away counts'),
                  body: t(
                    '应用运行时，锁屏或睡眠会计入休息。休息足够长，就重新安排下一轮；短暂离开，则保留之前的工作进度。',
                    'While the app is running, time locked or asleep counts as rest. A long enough rest resets the next work cycle; a brief absence preserves your progress.',
                  ),
                },
              ].map(({ Icon, title, body }) => (
                <article key={title}>
                  <Icon aria-hidden="true" />
                  <h3>{title}</h3>
                  <p>{body}</p>
                </article>
              ))}
            </div>
            <p className="daily-details">
              {t(
                '还有这些日常小事：系统通知、可选提示音、深浅色外观，以及关掉窗口后继续在菜单栏运行。',
                'The everyday details, too: system notifications, optional chimes, light and dark themes, and a menu-bar companion that keeps running when you close the window.',
              )}
            </p>
          </div>
        </section>
        <section className="phone-section shell" id="phone-key">
          <div className="phone-copy">
            <p className="eyebrow">OUTSIE PHONE KEY</p>
            <h2>
              {t('你带走手机。', 'Take your phone.')}
              <br />
              <em>{t('我们照看屏幕。', 'Leave the screen to us.')}</em>
            </h2>
            <p>
              {t(
                '把手机变成 Mac 的随身钥匙。带着手机离开，电脑自动锁屏；回到桌前，验证后自动解锁。起身休息，少一点牵挂。',
                'Your phone, a key to your Mac. Walk away to lock your screen; return to unlock after verification. One less thing to remember when you step away.',
              )}
            </p>
            <a className="text-link" href="#status">
              {t('看看其他功能', 'Explore more features')}
              <ArrowRight size={17} aria-hidden="true" />
            </a>
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
                    '用手机扫描 Mac 上的一次性二维码，开始配对。',
                    'Scan the one-time QR code on your Mac to start pairing.',
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
        <section className="status-section" id="status">
          <div className="shell status-grid">
            <div>
              <p className="eyebrow">BUILT AROUND THE HUMAN</p>
              <h2>{t('把身体也放进工作流程。', 'Make room for the human.')}</h2>
              <p className="status-intro">
                {t(
                  '该停时停下来，该动时起身，继续工作时也给双手减负。Outsie 把这些小事放在一起，帮你找回自己的节奏。',
                  'Pause when it’s time, get up and move, and give your hands less repetitive work when you continue. Outsie brings the little things together so you can find your own rhythm.',
                )}
              </p>
            </div>
            <div className="status-list">
              {[
                {
                  Icon: Sprout,
                  title: t(
                    '强制休息与 3D 拉伸',
                    'Enforced breaks & 3D stretching',
                  ),
                  detail: t(
                    '短休息、长休息、多显示器覆盖，跟着 3D 示范舒展身体。',
                    'Short and long breaks across every display, with 3D guidance to help you stretch.',
                  ),
                  tag: 'BREAKS',
                },
                {
                  Icon: Video,
                  title: t('开会时不打扰', 'Meetings come first'),
                  detail: t(
                    'Zoom、Teams、飞书、腾讯会议通话中，到点的休息等会议结束再来。会议时间单独记录，不算专注。',
                    'On a call in Zoom, Teams, Feishu or Tencent Meeting, a break that comes due waits for the call to end. Meeting time is recorded separately from focus.',
                  ),
                  tag: 'MEETINGS',
                },
                {
                  Icon: LockKeyhole,
                  title: t('闲置自动锁屏', 'Automatic idle lock'),
                  detail: t(
                    '启用后，系统键鼠闲置 30 秒自动锁屏。',
                    'Enable idle locking to lock your Mac after 30 seconds without keyboard or mouse activity.',
                  ),
                  tag: 'AUTO LOCK',
                },
                {
                  Icon: Smartphone,
                  title: t('手机钥匙', 'Phone Key'),
                  detail: t(
                    '扫码配对，校准距离。离开锁屏，回来验证后继续。',
                    'Pair with a scan and calibrate the distance. Lock when you leave; verify and resume when you return.',
                  ),
                  tag: 'PHONE KEY',
                },
                {
                  Icon: Headphones,
                  title: t(
                    '手机控制与语音协作',
                    'Phone controls & voice collaboration',
                  ),
                  detail: t(
                    '手机选操作，耳机说想法，少些重复键鼠操作。',
                    'Choose actions on your phone and speak your ideas through a headset. Less repetitive keyboard and mouse work.',
                  ),
                  tag: 'PHONE + VOICE',
                },
              ].map(({ Icon, title, detail, tag }) => (
                <article key={title}>
                  <span className="status-icon ready">
                    <Icon size={18} aria-hidden="true" />
                  </span>
                  <div>
                    <h3>{title}</h3>
                    <p>{detail}</p>
                  </div>
                  <span className="status-tag ready">{tag}</span>
                </article>
              ))}
            </div>
          </div>
        </section>
        <section
          className="download-section shell"
          id="download"
          aria-labelledby="download-title"
        >
          <div className="download-copy">
            <p className="eyebrow">A LITTLE SPACE, ON YOUR MAC.</p>
            <h2 id="download-title">
              {t('把休息，留进日常。', 'Make room for a daily pause.')}
            </h2>
            <p>
              {t(
                '下载 Mac 应用，让按时休息和起身拉伸，成为工作的一部分。',
                'Bring scheduled breaks and guided stretches into your working day.',
              )}
            </p>
            <div className="download-actions">
              <a className="primary-cta" href={macRelease.downloadUrl}>
                <Download size={18} aria-hidden="true" />
                {t('下载 Mac 安装包', 'Download for Mac')}
              </a>
              <a className="primary-cta" href={macRelease.androidDownloadUrl}>
                <Smartphone size={18} aria-hidden="true" />
                {t('下载 Android 版（手机钥匙）', 'Download for Android (Phone Key)')}
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
            <p className="download-scope">
              {t(
                'Mac 版包含休息、拉伸和手机钥匙；Android 版是钥匙本身，配对后走近解锁、走远锁屏、点一下按快捷键。',
                'The Mac app includes breaks, stretching and Phone Key; the Android app is the key itself: pair once, then unlock as you walk up, lock as you walk away, and press shortcuts with a tap.',
              )}
            </p>
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
            <p className="eyebrow">HUMAN QUESTIONS</p>
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
                t('手机钥匙怎么使用？', 'How does Phone Key work?'),
                t(
                  '在 Mac 上打开配对二维码，用手机扫码，再校准靠近和离开的距离。之后带着手机离开时锁屏，回来时验证手机钥匙并恢复会话。重启后的首次登录仍使用系统验证。',
                  'Scan the pairing QR code on your Mac with your phone, then calibrate nearby and away signals. Take your phone with you to lock your screen; return to verify your key and resume your session. The first login after a restart still uses system authentication.',
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
                t('手机工作台能做什么？', 'What can I do with Phone Controls?'),
                t(
                  '在手机上选择 App，就能使用对应的操作面板：给终端分屏、切换 Codex 任务、打开常用协作入口。常用快捷键和多步按键都能放到按钮里，配合耳机语音输入，减少回到键盘鼠标前的次数。',
                  'Choose an app on your phone to bring up its actions: split terminal panes, switch Codex tasks, or open everyday collaboration tools. Put shortcuts and key sequences on buttons, and pair them with headset voice input for fewer trips back to the keyboard and mouse.',
                ),
              ],
              [
                t('为什么还要用手机和耳机？', 'Why a phone and headset?'),
                t(
                  '出发点是减少手部负担。让手机承担少量选择和控制，语音承担长段输入，帮助你离开键盘鼠标、换个姿势继续和 AI 协作。目标是少些重复敲击和点击；到点休息时，再让眼睛和身体一起停下来。',
                  'The goal is to give your hands less repetitive work: a few choices on the phone, longer input by voice. That lets you change position and collaborate with AI away from the keyboard and mouse. Scheduled breaks still give your eyes and body time away from work.',
                ),
              ],
              [
                t('是不是只有程序员能用？', 'Is Outsie only for developers?'),
                t(
                  '任何长时间坐在屏幕前的人都可以使用休息功能。AI Coding 是我们的出发点：总有下一条 prompt，也该有下一次休息。',
                  'Anyone who spends long stretches at a screen can use the break features. AI coding is where our story starts: there’s always another prompt. There should be another break, too.',
                ),
              ],
              [
                t(
                  '需要什么设备？在哪里下载？',
                  'What devices do I need? Where can I download it?',
                ),
                t(
                  `从本页“下载 Mac 版”可获取安装包。目前提供 v${macRelease.version}，适用于 Apple Silicon 芯片的 Mac，系统需为 macOS 14 或更新版本。安装后应用名为 Outsie，包含休息、拉伸、开会时不打扰、手机钥匙和手机工作台；手机那一端要另装 Android 版，需要 Android 12 或更新版本。`,
                  `Use “Download for Mac” on this page to get v${macRelease.version} for Apple Silicon Macs running macOS 14 or later. The installed app is named Outsie and includes breaks, stretching, meeting-aware breaks, Phone Key and Phone Controls. Phone Key also needs the Android app on a phone running Android 12 or later.`,
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
          <p className="eyebrow">YOUR AI HAS NO BODY. YOU DO.</p>
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
          <a href="#status">{t('功能一览', 'Features')}</a>
          <span>© {new Date().getFullYear()} Outsie</span>
        </footer>
      </main>
    </div>
  );
}
