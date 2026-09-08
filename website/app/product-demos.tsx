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
            '浏览器示意 · Mac 应用已包含护眼知识',
            'Browser illustration · Eye-care tips are included in the Mac app',
          )}
        </span>
      </figcaption>
      <div
        className="rest-preview"
        aria-label={t('休息界面示意', 'Break screen preview')}
      >
        <Eye size={30} strokeWidth={1.5} aria-hidden="true" />
        <p>{t('抬头，看看远处。', 'Look up. Look further.')}</p>
        <div className="rest-eye-tip">
          <b>{t('护眼小知识 · 看远处，让对焦歇一会', 'Eye-care tip · Give near focus a break')}</b>
          <span>{t('每近距离用眼约 20 分钟，看向约 6 米外至少 20 秒。', 'Every 20 minutes, look about 20 feet away for at least 20 seconds.')}</span>
        </div>
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
            'Mac 应用中的离线 3D 下巴微收示范',
            'Offline chin-tuck guide from the Mac app',
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
          <strong>05:00</strong>
          <span>{t('大休息剩余 · 每 30 秒换动作', 'break remaining · a new movement every 30 sec')}</span>
        </div>
        <span className="demo-footnote">
          {t(
            '浏览器静态示意 · Mac 应用内为实时 3D 跟练',
            'Static browser illustration · Live 3D guidance in the Mac app',
          )}
        </span>
      </figcaption>
    </figure>
  );
}

export { PhoneKeyDemo } from './phone-key-demo';
