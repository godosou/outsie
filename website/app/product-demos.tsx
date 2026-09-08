'use client';

import Image from 'next/image';
import { publicAsset } from '@/lib/public-asset';
import { ArrowRight, Eye } from 'lucide-react';

type DemoProps = { lang: 'zh' | 'en' };

export function BreakDemo({ lang }: DemoProps) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  return (
    <figure className="simple-break-demo" id="app-preview">
      <figcaption className="break-demo-copy">
        <span className="demo-kicker">A LITTLE PAUSE</span>
        <h3>{t('到点了，先歇一下。', 'Time for a little pause.')}</h3>
        <p>
          {t(
            '屏幕先停一会儿。抬抬头，让眼睛看看远处。',
            'Let the screen wait. Look up, and give your eyes a change of scenery.',
          )}
        </p>
        <div className="simple-rhythm">
          <span>
            <strong>20</strong>
            {t('分钟工作', 'min of focus')}
          </span>
          <ArrowRight aria-hidden="true" />
          <span>
            <strong>20</strong>
            {t('秒钟休息', 'sec to pause')}
          </span>
        </div>
        <span className="demo-footnote">
          {t(
            '休息示意 · 时间可按你的习惯调整',
            'Break preview · Adjust the timing to your rhythm',
          )}
        </span>
      </figcaption>
      <div
        className="rest-preview"
        aria-label={t('休息界面示意', 'Break screen preview')}
      >
        <Eye size={30} strokeWidth={1.5} aria-hidden="true" />
        <p>{t('抬头，看看远处。', 'Look up. Look further.')}</p>
        <strong className="rest-time">00:20</strong>
        <span>{t('这 20 秒，留给自己。', 'These 20 seconds are yours.')}</span>
      </div>
    </figure>
  );
}

export function StretchDemo({ lang }: DemoProps) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  return (
    <figure className="simple-stretch-demo">
      <div className="stretch-scene-preview">
        <Image
          src={publicAsset('/demos/stretch-focus.webp')}
          width={720}
          height={640}
          alt={t(
            '现有 3D 引导中的下巴微收动作示范',
            'Chin-tuck movement from the existing 3D guide',
          )}
          unoptimized
        />
      </div>
      <figcaption className="stretch-demo-copy">
        <span className="demo-kicker">
          01 / 08 · {t('拉伸跟练', 'STRETCH GUIDE')}
        </span>
        <h3>{t('下巴，轻轻收回来。', 'Gently tuck your chin.')}</h3>
        <p>
          {t(
            '目视前方，下巴缓缓向后收。肩膀放松，按自己舒服的幅度来。',
            'Look ahead and gently draw your chin back. Relax your shoulders and stay within a comfortable range.',
          )}
        </p>
        <div className="stretch-duration">
          <strong>30</strong>
          <span>{t('秒 / 一个动作', 'sec / one movement')}</span>
        </div>
        <span className="demo-footnote">
          {t(
            '现有 3D 动作 · 首页简化展示',
            'Existing 3D movement · Simplified preview',
          )}
        </span>
      </figcaption>
    </figure>
  );
}

export { PhoneKeyDemo } from './phone-key-demo';
