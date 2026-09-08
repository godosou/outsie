'use client';

import { useEffect, useRef, useState } from 'react';
import Image from 'next/image';
import {
  BatteryFull,
  Check,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  Monitor,
  Pause,
  Play,
  RotateCcw,
  ShieldCheck,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { publicAsset } from '@/lib/public-asset';
import './phone-key-demo.css';

const duration = 10_000;

export function PhoneKeyDemo({ lang }: { lang: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  const container = useRef<HTMLElement>(null);
  const started = useRef(false);
  const [scenario, setScenario] = useState<'work' | 'break'>('work');
  const [elapsed, setElapsed] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [visible, setVisible] = useState(false);
  const [pageVisible, setPageVisible] = useState(true);
  const finished = elapsed >= duration;
  const phase =
    elapsed < 1800
      ? 'near'
      : elapsed < 4500
        ? 'away'
        : elapsed < 6200
          ? 'verifying'
          : 'returned';
  const locked = phase === 'away' || phase === 'verifying';
  const breakSeconds = 20 - Math.floor(elapsed / 1000);

  useEffect(() => {
    const node = container.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        const onScreen = entry.isIntersecting && entry.intersectionRatio >= 0.3;
        setVisible(onScreen);
        if (onScreen && !started.current) {
          started.current = true;
          if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
            setPlaying(true);
          }
        }
      },
      { threshold: 0.3 },
    );
    observer.observe(node);
    const onVisibility = () =>
      setPageVisible(document.visibilityState === 'visible');
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      observer.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  useEffect(() => {
    if (!playing || !visible || !pageVisible || finished) return;
    let previous = performance.now();
    const timer = window.setInterval(() => {
      const now = performance.now();
      const delta = now - previous;
      previous = now;
      setElapsed((current) => Math.min(duration, current + delta));
    }, 100);
    return () => window.clearInterval(timer);
  }, [playing, visible, pageVisible, finished]);

  function chooseScenario(next: 'work' | 'break') {
    started.current = true;
    setScenario(next);
    setElapsed(0);
    setPlaying(true);
  }

  function togglePlayback() {
    started.current = true;
    if (finished) {
      setElapsed(0);
      setPlaying(true);
    } else {
      setPlaying((current) => !current);
    }
  }

  const status = {
    near:
      scenario === 'break'
        ? t(
            '休息开始，带上手机走一走。',
            'Your break has started. Take your phone and stretch your legs.',
          )
        : t(
            '手机就在身边，屏幕照常使用。',
            'Your phone is nearby. Your Mac is ready.',
          ),
    away: t(
      '带着手机离开，电脑自动锁屏。',
      'Walk away with your phone. Your Mac locks.',
    ),
    verifying: t(
      '回到桌前，正在验证手机钥匙。',
      'Back at your desk. Verifying your Phone Key.',
    ),
    returned:
      scenario === 'break'
        ? t(
            '验证通过，休息倒计时继续，不会被跳过。',
            'Verified. Your break keeps counting down—it is not skipped.',
          )
        : t(
            '验证通过，回到刚才的工作。',
            'Verified. Pick up where you left off.',
          ),
  }[phase];
  const playbackLabel = finished
    ? t('再看一次', 'Replay')
    : playing
      ? t('暂停', 'Pause')
      : t('播放演示', 'Play demo');
  const PlaybackIcon = finished ? RotateCcw : playing ? Pause : Play;

  return (
    <figure
      className="phone-key-demo"
      ref={container}
      data-phase={phase}
      data-playing={playing && visible && pageVisible && !finished}
    >
      <div className="key-demo-toolbar">
        <fieldset className="key-scenarios">
          <legend className="sr-only">
            {t('选择演示场景', 'Choose a demo scenario')}
          </legend>
          <Button
            variant="ghost"
            aria-pressed={scenario === 'work'}
            onClick={() => chooseScenario('work')}
          >
            {t('日常离开', 'Step away')}
          </Button>
          <Button
            variant="ghost"
            aria-pressed={scenario === 'break'}
            onClick={() => chooseScenario('break')}
          >
            {t('休息中返回', 'Return during a break')}
          </Button>
        </fieldset>
        <Button
          className="key-demo-play"
          variant="ghost"
          onClick={togglePlayback}
          aria-label={playbackLabel}
        >
          <PlaybackIcon size={16} aria-hidden="true" />
          {playbackLabel}
        </Button>
      </div>

      <div className="key-device-scene" aria-hidden="true">
        <div className="key-phone-area">
          <div className="key-phone-device">
            <div className="key-phone-top">
              <span>9:41</span>
              <i />
              <BatteryFull size={17} />
            </div>
            <div className="key-phone-content">
              <span className="key-phone-brand">outsie.</span>
              <div className="key-phone-symbol">
                <KeyRound />
              </div>
              <strong>
                {phase === 'away'
                  ? t('已离开', 'Away')
                  : phase === 'verifying'
                    ? t('验证中', 'Verifying')
                    : t('在你身边', 'Nearby')}
              </strong>
              <span className="key-phone-subtitle">PHONE KEY</span>
              <div className="key-paired-mac">
                <Monitor size={18} />
                <span>{t('我的 Mac', 'My Mac')}</span>
                <Check size={15} />
              </div>
              <span className="key-phone-home" />
            </div>
          </div>
          <span className="key-distance-label">
            {phase === 'away'
              ? t('带着手机，走远一点', 'Take your phone with you')
              : t('手机就在电脑旁', 'Phone beside your Mac')}
          </span>
        </div>

        <div className="key-connection">
          <span />
          <span />
          <span />
          <ShieldCheck size={22} />
          <span />
          <span />
          <span />
        </div>

        <div className="key-mac-device">
          <div className="key-mac-screen">
            <div className="key-mac-menubar">
              <span className="key-window-dots">
                <i />
                <i />
                <i />
              </span>
              <strong>Outsie</strong>
              <span>9:41</span>
            </div>
            <div
              className="key-mac-desktop"
              data-resting={scenario === 'break'}
            >
              {scenario === 'break' ? (
                <div className="key-mac-rest">
                  <Image
                    src={publicAsset('/outsie.png')}
                    width={64}
                    height={64}
                    alt=""
                    unoptimized
                  />
                  <strong>
                    {phase === 'returned'
                      ? t('休息还在继续。', 'Your break continues.')
                      : t('先歇一会儿。', 'Time for a pause.')}
                  </strong>
                  <span className="key-rest-countdown">
                    00:{String(breakSeconds).padStart(2, '0')}
                  </span>
                  <span>
                    {t('这段时间，留给自己。', 'This time is still yours.')}
                  </span>
                </div>
              ) : (
                <div className="key-work-window">
                  <div>
                    <span>{t('今天的工作', 'Today’s work')}</span>
                    <span>
                      <Check size={13} /> {t('已保存', 'Saved')}
                    </span>
                  </div>
                  <h3>
                    {t('想法还在，工作也在。', 'Right where you left it.')}
                  </h3>
                  <div className="key-work-lines">
                    <i />
                    <i />
                    <i />
                    <i />
                  </div>
                  <span className="key-work-cursor" />
                </div>
              )}
              <div className="key-mac-dock">
                <i />
                <i />
                <i />
                <i />
              </div>
            </div>
            <div className="key-lock-screen" data-active={locked}>
              {phase === 'verifying' ? (
                <LoaderCircle className="key-verifying-icon" size={29} />
              ) : (
                <LockKeyhole size={29} />
              )}
              <span className="key-lock-time">09:41</span>
              <strong>
                {phase === 'verifying'
                  ? t('正在验证手机钥匙', 'Verifying Phone Key')
                  : t('屏幕已锁定', 'Screen locked')}
              </strong>
              <span>
                {phase === 'verifying'
                  ? t('确认是你，再打开屏幕。', 'Verify first. Then unlock.')
                  : t(
                      '放心走开，这里已锁好。',
                      'Your screen is taken care of.',
                    )}
              </span>
            </div>
            <span
              className="key-verified-badge"
              data-visible={phase === 'returned'}
            >
              <ShieldCheck size={15} />
              {t('手机钥匙已验证', 'Phone Key verified')}
            </span>
          </div>
          <div className="key-mac-base">
            <span />
          </div>
        </div>
      </div>

      <div className="key-demo-story">
        <ol aria-label={t('演示进度', 'Demo progress')}>
          {[
            t('带走手机', 'Walk away'),
            t('回来验证', 'Return & verify'),
            scenario === 'break'
              ? t('继续休息', 'Keep resting')
              : t('恢复屏幕', 'Resume'),
          ].map((label, index) => {
            const active =
              index ===
              (phase === 'near' || phase === 'away'
                ? 0
                : phase === 'verifying'
                  ? 1
                  : 2);
            return (
              <li key={index} aria-current={active ? 'step' : undefined}>
                <span>{String(index + 1).padStart(2, '0')}</span>
                {label}
              </li>
            );
          })}
        </ol>
        <output aria-live="polite" aria-atomic="true">
          {status}
        </output>
      </div>
      <figcaption>
        {t(
          '交互演示 · 不会连接设备或锁定你的电脑',
          'Interactive demo · Does not connect to devices or lock your Mac',
        )}
      </figcaption>
    </figure>
  );
}
