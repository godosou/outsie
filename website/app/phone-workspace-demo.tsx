'use client';

import { useState } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  BatteryFull,
  Check,
  Code2,
  Columns2,
  FileCode2,
  Layers2,
  Maximize2,
  Monitor,
  Plus,
  RotateCcw,
  Rows2,
  Smartphone,
  Terminal,
  Wifi,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';

export function PhoneWorkspaceDemo({ lang }: { lang: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => (lang === 'zh' ? zh : en);
  const [app, setApp] = useState('tmux');
  const [layout, setLayout] = useState<'single' | 'columns' | 'rows'>('single');
  const [activePane, setActivePane] = useState(1);
  const [zoom, setZoom] = useState(false);
  const [windowId, setWindowId] = useState(1);
  const [taskIndex, setTaskIndex] = useState(0);
  const [draft, setDraft] = useState(false);
  const [feedback, setFeedback] = useState('ready');
  const tasks = [
    t('新任务', 'New task'),
    t('手机工作台设计', 'Phone workspace'),
    t('修复计时器', 'Fix the timer'),
  ];
  const feedbackText: Record<string, string> = {
    ready: t('试试手机上的「左右分屏」。', 'Try “Split panes” on the phone.'),
    columns: t('电脑已左右分屏。', 'Your Mac now has two side-by-side panes.'),
    rows: t('电脑已上下分屏。', 'Your Mac now has two stacked panes.'),
    pane: t('已切换到另一个窗格。', 'Switched to the other pane.'),
    zoom: t('已放大当前窗格。', 'Focused on the current pane.'),
    restore: t('已恢复两个窗格。', 'Both panes are visible again.'),
    sequence: t(
      '新建窗口 → 分屏，两步完成。',
      'New window → split. Both steps done.',
    ),
    codex: t('电脑已切换到 Codex。', 'Switched your Mac to Codex.'),
    tmux: t('电脑已切换到 tmux。', 'Switched your Mac to tmux.'),
    task: t('电脑已切换任务。', 'Switched tasks on your Mac.'),
    new: t('新任务已准备好。', 'Your new task is ready.'),
    draft: t(
      '代码审查草稿已准备，尚未发送。',
      'Review draft ready. Nothing has been sent.',
    ),
  };
  function split(direction: 'columns' | 'rows') {
    setLayout(direction);
    setActivePane(2);
    setZoom(false);
    setFeedback(direction);
  }
  function reset() {
    setApp('tmux');
    setLayout('single');
    setActivePane(1);
    setZoom(false);
    setWindowId(1);
    setTaskIndex(0);
    setDraft(false);
    setFeedback('ready');
  }
  const tmuxActions = [
    {
      Icon: Columns2,
      label: t('左右分屏', 'Split panes'),
      onClick: () => split('columns'),
      selected: layout === 'columns' && !zoom,
    },
    {
      Icon: Rows2,
      label: t('上下分屏', 'Stack panes'),
      onClick: () => split('rows'),
      selected: layout === 'rows' && !zoom,
    },
    {
      Icon: Layers2,
      label: t('切换窗格', 'Switch pane'),
      onClick: () => {
        setActivePane(activePane === 1 ? 2 : 1);
        setFeedback('pane');
      },
      disabled: layout === 'single',
    },
    {
      Icon: Maximize2,
      label: zoom ? t('恢复窗格', 'Restore panes') : t('放大窗格', 'Zoom pane'),
      onClick: () => {
        setZoom(!zoom);
        setFeedback(zoom ? 'restore' : 'zoom');
      },
      disabled: layout === 'single',
      selected: zoom,
    },
  ];
  const codexActions = [
    {
      Icon: Plus,
      label: t('新建任务', 'New task'),
      onClick: () => {
        setTaskIndex(0);
        setDraft(false);
        setFeedback('new');
      },
    },
    {
      Icon: FileCode2,
      label: t('代码审查', 'Review code'),
      onClick: () => {
        setTaskIndex(0);
        setDraft(true);
        setFeedback('draft');
      },
    },
    {
      Icon: ArrowLeft,
      label: t('上个任务', 'Previous task'),
      onClick: () => {
        setTaskIndex((taskIndex + 2) % 3);
        setDraft(false);
        setFeedback('task');
      },
    },
    {
      Icon: ArrowRight,
      label: t('下个任务', 'Next task'),
      onClick: () => {
        setTaskIndex((taskIndex + 1) % 3);
        setDraft(false);
        setFeedback('task');
      },
    },
  ];
  const paneIds = zoom ? [activePane] : layout === 'single' ? [1] : [1, 2];

  return (
    <figure className="workspace-demo" id="workspace-demo">
      <figcaption className="workspace-demo-heading">
        <div>
          <span className="demo-kicker">
            {t('手机 → 电脑', 'PHONE → MAC')}
          </span>
          <h3>
            {t(
              '手机上的一步，电脑上的变化。',
              'A tap here. A change over there.',
            )}
          </h3>
        </div>
        <Button variant="ghost" className="workspace-reset" onClick={reset}>
          <RotateCcw aria-hidden="true" />
          {t('重置演示', 'Reset demo')}
        </Button>
      </figcaption>
      <Tabs
        value={app}
        onValueChange={(value) => {
          setApp(String(value));
          setFeedback(String(value));
        }}
        className="workspace-scene"
      >
        <div className="workspace-phone-column">
          <p className="workspace-device-label">
            <Smartphone aria-hidden="true" />
            {t('手机 · 选择操作', 'PHONE · CHOOSE AN ACTION')}
          </p>
          <div className="workspace-phone">
            <div className="workspace-phone-status" aria-hidden="true">
              <span>9:41</span>
              <span className="workspace-phone-island" />
              <span>
                <Wifi />
                <BatteryFull />
              </span>
            </div>
            <div className="workspace-phone-heading">
              <span>
                outsie<span>.</span>
              </span>
              <span>{t('工作台', 'Workspace')}</span>
            </div>
            <div className="workspace-device-card">
              <Monitor aria-hidden="true" />
              <div>
                <strong>MacBook Pro</strong>
                <span>{t('连接演示', 'Demo connection')}</span>
              </div>
              <span className="workspace-connection-dot" aria-hidden="true" />
            </div>
            <TabsList
              className="workspace-phone-apps"
              aria-label={t('选择演示中的电脑应用', 'Choose the demo app')}
            >
              <TabsTrigger value="tmux">
                <Terminal aria-hidden="true" />
                tmux
              </TabsTrigger>
              <TabsTrigger value="codex">
                <Code2 aria-hidden="true" />
                Codex
              </TabsTrigger>
            </TabsList>
            <p className="workspace-panel-label">
              {t('常用操作', 'QUICK ACTIONS')}
            </p>
            <TabsContent value="tmux" className="workspace-phone-actions">
              <div className="workspace-key-grid">
                {tmuxActions.map(
                  ({ Icon, label, onClick, disabled, selected }) => (
                    <Button
                      key={label}
                      className={`workspace-key ${selected ? 'is-selected' : ''}`}
                      onClick={onClick}
                      disabled={disabled}
                    >
                      <Icon aria-hidden="true" />
                      <span>{label}</span>
                    </Button>
                  ),
                )}
              </div>
              <Button
                className="workspace-sequence"
                onClick={() => {
                  setWindowId(2);
                  setLayout('columns');
                  setZoom(false);
                  setActivePane(2);
                  setFeedback('sequence');
                }}
              >
                <Layers2 aria-hidden="true" />
                <span>
                  <strong>{t('新建窗口并分屏', 'New window + split')}</strong>
                  <small>
                    {t('一个按钮，两步操作', 'One button. Two steps.')}
                  </small>
                </span>
                <ArrowRight aria-hidden="true" />
              </Button>
            </TabsContent>
            <TabsContent value="codex" className="workspace-phone-actions">
              <div className="workspace-key-grid">
                {codexActions.map(({ Icon, label, onClick }) => (
                  <Button
                    key={label}
                    className="workspace-key"
                    onClick={onClick}
                  >
                    <Icon aria-hidden="true" />
                    <span>{label}</span>
                  </Button>
                ))}
              </div>
              <p className="workspace-phone-hint">
                {t(
                  '手机选任务，电脑打开对应页面。',
                  'Choose a task here. Open it on your Mac.',
                )}
              </p>
            </TabsContent>
            <div className="workspace-home-indicator" aria-hidden="true" />
          </div>
        </div>
        <div className="workspace-link" aria-hidden="true">
          <ArrowRight />
        </div>
        <div className="workspace-mac-column">
          <p className="workspace-device-label">
            <Monitor aria-hidden="true" />
            {t('电脑 · 即时响应', 'MAC · SEE THE RESULT')}
          </p>
          <div className="workspace-mac">
            <div className="workspace-mac-bar">
              <span className="workspace-traffic" aria-hidden="true">
                <i />
                <i />
                <i />
              </span>
              <strong>{app === 'tmux' ? 'Terminal' : 'Codex'}</strong>
              <span>Outsie Workspace</span>
            </div>
            <div className="workspace-desktop">
              <div
                className={`workspace-app-window ${app === 'codex' ? 'is-codex' : ''}`}
              >
                <div className="workspace-window-bar">
                  <span>
                    {app === 'tmux' ? (
                      <Terminal aria-hidden="true" />
                    ) : (
                      <Code2 aria-hidden="true" />
                    )}
                    {app === 'tmux'
                      ? `tmux — outsie-dev:${windowId}`
                      : 'Codex — outsie'}
                  </span>
                  <span>
                    {app === 'tmux'
                      ? zoom
                        ? 'ZOOM'
                        : `${paneIds.length} ${t('个窗格', 'PANES')}`
                      : t('本地工作区', 'Local workspace')}
                  </span>
                </div>
                {app === 'tmux' ? (
                  <div
                    className={`workspace-terminal ${!zoom ? layout : 'single'}`}
                  >
                    {paneIds.map((id) => (
                      <div
                        key={id}
                        className={`workspace-pane ${activePane === id ? 'is-active' : ''}`}
                      >
                        <div className="workspace-pane-label">
                          <span>
                            {windowId}:%{id}
                          </span>
                          <span>{activePane === id ? '● ACTIVE' : 'zsh'}</span>
                        </div>
                        <p>
                          <span className="workspace-prompt">➜</span> outsie{' '}
                          <span className="workspace-dim">git:(main)</span>
                        </p>
                        <p className="workspace-terminal-welcome">
                          {id === 1
                            ? 'Welcome to your workspace.'
                            : 'A little more room to think.'}
                        </p>
                        <p>
                          <span className="workspace-prompt">$</span>{' '}
                          {activePane === id && (
                            <span
                              className="workspace-cursor"
                              aria-hidden="true"
                            />
                          )}
                        </p>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="workspace-codex">
                    <div className="workspace-task-list">
                      <span>{t('任务', 'TASKS')}</span>
                      {tasks.map((task, index) => (
                        <div
                          key={task}
                          className={taskIndex === index ? 'is-current' : ''}
                        >
                          {index === 0 ? (
                            <Plus aria-hidden="true" />
                          ) : (
                            <Code2 aria-hidden="true" />
                          )}
                          {task}
                        </div>
                      ))}
                    </div>
                    <div className="workspace-task-content">
                      <Code2 size={28} strokeWidth={1.5} aria-hidden="true" />
                      <span className="workspace-project-label">
                        outsie / {t('本地项目', 'local project')}
                      </span>
                      <h4>
                        {taskIndex === 0
                          ? t(
                              '今天，想做点什么？',
                              'What shall we build today?',
                            )
                          : tasks[taskIndex]}
                      </h4>
                      <div
                        className={`workspace-draft ${draft ? 'has-draft' : ''}`}
                      >
                        {draft
                          ? t(
                              '请审查当前代码，找出潜在问题并给出修改建议。',
                              'Review the current code, identify potential issues, and suggest improvements.',
                            )
                          : t('描述你的任务…', 'Describe your task…')}
                        <ArrowUp aria-hidden="true" />
                      </div>
                      <span className="workspace-draft-note">
                        {draft
                          ? t('草稿已准备 · 未发送', 'Draft ready · Not sent')
                          : t('等待你的想法', 'Ready for your ideas')}
                      </span>
                    </div>
                  </div>
                )}
              </div>
            </div>
            <div className="workspace-mac-chin">Outsie</div>
          </div>
          <output className="workspace-feedback">
            <Check aria-hidden="true" />
            <span>{feedbackText[feedback]}</span>
          </output>
        </div>
      </Tabs>
      <p className="workspace-demo-note">
        {t(
          '基于现有交互稿精简 · 页面内模拟，不连接真实设备',
          'Adapted from the existing prototype · Simulated here, with no real device connection',
        )}
      </p>
    </figure>
  );
}
