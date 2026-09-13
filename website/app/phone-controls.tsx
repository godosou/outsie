'use client';

import { ArrowRight, Check, Copy, Smartphone } from 'lucide-react';
import { PhoneWorkspaceDemo } from './phone-workspace-demo';

export function PhoneControls({ lang }: { lang: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  return (
    <section className="controls-section" id="phone-controls">
      <div className="shell">
        <div className="controls-heading">
          <div>
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
                '用手机切换任务、执行常用操作，配合耳机语音输入，把想法说给 AI。',
                'Switch tasks and run everyday actions from your phone, and use headset voice input to tell AI what you have in mind.',
              )}
            </p>
          </div>
        </div>
        <PhoneWorkspaceDemo lang={lang} />
        <div className="ai-config">
          <div className="ai-config-copy">
            <span className="feature-status">
              <span className="live-dot" />
              {t('快捷键设置', 'Shortcut settings')}
            </span>
            <h3>
              {t('这些按钮，', 'These buttons?')}
              <br />
              {t('让 AI 替你配。', 'Let an AI set them up.')}
            </h3>
            <p>
              {t(
                '同类软件配快捷键要一项项手填。这里复制一段说明给会跑命令的 AI，说要什么按钮，它就改好。',
                'Most apps make you fill in a shortcut table row by row. Here you copy one block of instructions into an AI that can run commands, say which buttons you want, and it does the rest.',
              )}
            </p>
          </div>
          <ol className="ai-config-flow">
            <li>
              <span className="ai-config-step">01</span>
              <div className="ai-config-card ai-config-mac">
                <div className="ai-config-bar">
                  <span className="ai-config-dots" aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </span>
                  <span>{t('快捷键设置', 'Shortcut settings')}</span>
                </div>
                <span className="ai-config-button">
                  <Copy size={14} aria-hidden="true" />
                  {t('复制给 AI 的说明', 'Copy instructions for AI')}
                </span>
                <span className="ai-config-hint">
                  {t('说明自带你现在的配置', 'Carries your current setup')}
                </span>
              </div>
            </li>
            <li>
              <span className="ai-config-step">02</span>
              <div className="ai-config-card ai-config-say">
                <q>{t('给飞书加个搜索。', 'Add search to Feishu.')}</q>
                <span className="ai-config-hint">
                  {t(
                    '贴给 Claude Code、Codex…',
                    'Paste into Claude Code, Codex…',
                  )}
                </span>
              </div>
            </li>
            <li>
              <span className="ai-config-step">03</span>
              <div className="ai-config-card ai-config-result">
                <span className="ai-config-chip">
                  <Check size={13} aria-hidden="true" />
                  {t('飞书 · 搜索', 'Feishu · Search')}
                  <code>⌘K</code>
                </span>
                <span className="ai-config-hint">
                  <Smartphone size={13} aria-hidden="true" />
                  {t('手机上同步一次就有了', 'Sync once on your phone')}
                </span>
              </div>
            </li>
          </ol>
        </div>
        <p className="controls-note">
          <ArrowRight size={15} aria-hidden="true" />
          {t(
            '也可以把一串按键录成一个按钮。改完在手机上同步一次；按键需要 macOS 辅助功能权限，手机只报「已发出」，不报「已按下」。',
            'You can also record a key sequence onto a single button. Sync once on your phone afterwards. Pressing keys needs macOS Accessibility permission, and the phone reports “sent”, not “pressed”.',
          )}
        </p>
      </div>
    </section>
  );
}
