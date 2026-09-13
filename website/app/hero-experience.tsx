'use client';

import Image from 'next/image';
import { publicAsset } from '@/lib/public-asset';
import {
  ArrowRight,
  Headphones,
  Pause,
  Play,
  Plus,
  RotateCcw,
} from 'lucide-react';
import { Button } from '@/components/ui/button';

type HeroExperienceProps = {
  lang: 'zh' | 'en';
  seconds: number;
  running: boolean;
  onToggleBreak: () => void;
};

export function HeroExperience({
  lang,
  seconds,
  running,
  onToggleBreak,
}: HeroExperienceProps) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  return (
    <div className="hero-experience">
      <article className="hero-scenario hero-work-scenario">
        <div className="hero-scenario-label">
          <span>{t('少动手', 'LESS TYPING')}</span>
        </div>
        <h2>
          {t('手机选操作，耳机说想法。', 'Choose with a tap. Say the rest.')}
        </h2>
        <a className="hero-work-preview" href="#workspace-demo">
          <div className="hero-device-preview" aria-hidden="true">
            <div className="hero-mini-phone">
              <span className="hero-mini-phone-top" />
              <span className="hero-mini-brand">outsie.</span>
              <span className="hero-mini-action">
                <Plus />
                {t('新建任务', 'New task')}
              </span>
            </div>
            <div className="hero-mini-mac">
              <div className="hero-mini-mac-bar">
                <span className="hero-window-controls">
                  <i />
                  <i />
                  <i />
                </span>
                <span>Codex</span>
              </div>
              <div className="hero-mini-draft">
                <span>{t('输入示意', 'EXAMPLE INPUT')}</span>
                <q>{t('帮我看看这个改动。', 'Take a look at this change.')}</q>
                <span className="hero-mini-voice">
                  <Headphones />
                  {t('耳机语音输入', 'Headset voice input')}
                </span>
              </div>
            </div>
          </div>
          <span className="hero-preview-link">
            {t('体验手机与电脑联动', 'Try phone-to-Mac controls')}
            <ArrowRight size={17} aria-hidden="true" />
          </span>
        </a>
      </article>
      <article
        className={`hero-scenario hero-rest-scenario ${running ? 'is-running' : ''}`}
        id="break-demo"
      >
        <div className="hero-scenario-label">
          <span>{t('少久坐', 'LESS SITTING')}</span>
        </div>
        <h2>
          {seconds === 0
            ? t('歇好了，再继续。', 'Rested. Ready when you are.')
            : t(
                '到点停下来，真的歇一会儿。',
                'Pause on time. Take a real break.',
              )}
        </h2>
        <div className="hero-rest-preview">
          <Image
            className="hero-rest-flower"
            src={publicAsset('/outsie.png')}
            width={512}
            height={512}
            unoptimized
            priority
            alt={t('Outsie 的微笑小花', 'Outsie’s smiling flower')}
          />
          <div className="hero-rest-clock">
            <div
              className="hero-countdown"
              aria-label={t(
                `剩余 ${seconds} 秒`,
                `${seconds} seconds remaining`,
              )}
            >
              00:<span>{String(seconds).padStart(2, '0')}</span>
            </div>
            <p>
              {seconds === 0
                ? t(
                    '欢迎回来，别又坐忘了时间。',
                    'Welcome back. Keep making room for yourself.',
                  )
                : t(
                    '放松肩膀，把视线移开。',
                    'Drop your shoulders. Look away from the screen.',
                  )}
            </p>
          </div>
        </div>
        <div className="hero-rest-actions">
          <Button
            variant="outline"
            className="hero-timer-button"
            onClick={onToggleBreak}
          >
            {seconds === 0 ? (
              <RotateCcw aria-hidden="true" />
            ) : running ? (
              <Pause aria-hidden="true" />
            ) : (
              <Play aria-hidden="true" />
            )}
            {seconds === 0
              ? t('再体验一次', 'Try again')
              : running
                ? t('暂停演示', 'Pause demo')
                : seconds < 20
                  ? t('继续休息', 'Resume break')
                  : t('先歇 20 秒', 'Take 20 seconds')}
          </Button>
          <span>{t('网页演示，不会锁屏', 'Web demo · No screen locking')}</span>
        </div>
        <div className="hero-rest-progress" aria-hidden="true">
          <span style={{ width: `${(20 - seconds) * 5}%` }} />
        </div>
        <output className="sr-only" aria-live="polite">
          {seconds === 0
            ? t(
                '休息演示结束。欢迎回来。',
                'Demo break complete. Welcome back.',
              )
            : ''}
        </output>
      </article>
    </div>
  );
}
