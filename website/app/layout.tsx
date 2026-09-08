import type { Metadata } from 'next';
import { publicAsset } from '@/lib/public-asset';
import './globals.css';

export const metadata: Metadata = {
  metadataBase: new URL(process.env.SITE_URL ?? 'https://godosou.github.io/outsie/'),
  title: 'Outsie — AI 时代，先照顾好自己。',
  description:
    'AI 时代，先照顾好自己。别让 AI 把你榨干。Outsie 帮你按时休息、起身拉伸，用手机和耳机语音与 AI 协作。少坐一会儿，少敲点键盘。',
  icons: { icon: publicAsset('/favicon.svg') },
  openGraph: {
    title: 'Outsie — AI 时代，先照顾好自己。',
    description:
      'AI 时代，先照顾好自己。别让 AI 把你榨干。按时休息、起身拉伸，用手机和耳机语音与 AI 协作。',
    type: 'website',
    locale: 'zh_CN',
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-CN">
      <body>{children}</body>
    </html>
  );
}
