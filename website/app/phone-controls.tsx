'use client';

import { Grip, Headphones, Keyboard, ListOrdered, Monitor } from 'lucide-react';
import { PhoneWorkspaceDemo } from './phone-workspace-demo';

export function PhoneControls({ lang }: { lang: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  return (
    <section className="controls-section" id="phone-controls">
      <div className="shell">
        <div className="controls-heading">
          <div>
            <p className="eyebrow">LESS TYPING. MORE ROOM TO MOVE.</p>
            <h2>
              {t('离开键鼠，', 'Less typing.')}
              <br />
              {t('让手歇一会儿。', 'Give your hands a break.')}
            </h2>
          </div>
          <div className="controls-intro">
            <span className="feature-status">
              {t(
                '手机控制＋耳机语音输入',
                'Phone controls + headset voice input',
              )}
            </span>
            <p>
              {t(
                '用手机切换任务、执行常用操作，配合耳机语音输入，把想法说给 AI。离开键盘和鼠标，换个舒服的姿势，少些重复敲击，给双手减负。',
                'Switch tasks and run everyday actions from your phone. Use headset voice input to tell AI what you have in mind. Step away from the keyboard and mouse, change position, and give your hands less repetitive work.',
              )}
            </p>
          </div>
        </div>
        <PhoneWorkspaceDemo lang={lang} />
        <div className="voice-concept">
          <Headphones aria-hidden="true" />
          <div>
            <h3>
              {t('手机选操作，耳机说想法。', 'Tap to choose. Speak to create.')}
            </h3>
            <p>
              {t(
                '配合语音输入，少打长段文字。换个姿势，继续和 AI 协作。',
                'Add voice input for less typing. Change position and keep collaborating with AI.',
              )}
            </p>
          </div>
          <span>
            {t('少打字，给双手减负', 'Less typing. Lighter on your hands.')}
          </span>
        </div>
        <div className="controls-features">
          {[
            {
              Icon: Keyboard,
              title: t(
                '先用预置，再改成顺手的',
                'Start with presets. Make them yours.',
              ),
              body: t(
                '常用 App 先配好按钮，也可以改名称、图标和快捷键，按自己的习惯增删。',
                'Start with app presets, then rename, customize, add, or remove buttons to suit your habits.',
              ),
            },
            {
              Icon: ListOrdered,
              title: t(
                '一串按键，少做几次重复动作',
                'A key sequence. Less repetition.',
              ),
              body: t(
                '在 Mac 上录好常用按键序列，手机点一次就能触发，减少反复按组合键。',
                'Prepare frequent key sequences on your Mac and trigger them with one tap on your phone, reducing repeated key combinations.',
              ),
            },
            {
              Icon: Grip,
              title: t('怎么顺手，就怎么摆', 'Arrange it your way'),
              body: t(
                '手机按钮可以拖动排序，每个 App 单独保存布局，组合操作也能移动。',
                'Reorder buttons by dragging. Each app keeps its own layout, including buttons that run a sequence.',
              ),
            },
            {
              Icon: Monitor,
              title: t(
                '电脑切到哪，面板跟到哪',
                'Let the panel follow your app',
              ),
              body: t(
                '可以跟随电脑前台 App 切换，也可以固定面板。编辑布局时，面板保持不动。',
                'Follow the foreground app on your Mac or keep a panel pinned. Layout editing keeps the panel in place.',
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
  );
}
