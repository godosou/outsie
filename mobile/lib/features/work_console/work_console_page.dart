import 'package:flutter/material.dart';
import 'dart:async';
import 'console_controller.dart';
import 'console_models.dart';

String consoleText(BuildContext context, String en, String zh) =>
    Localizations.localeOf(context).languageCode == 'zh' ? zh : en;

class WorkConsolePage extends StatefulWidget {
  const WorkConsolePage({this.controller, super.key});
  final ConsoleController? controller;
  @override
  State<WorkConsolePage> createState() => _WorkConsolePageState();
}

class _WorkConsolePageState extends State<WorkConsolePage>
    with WidgetsBindingObserver {
  late final controller = widget.controller ?? ConsoleController();
  String t(String en, String zh) => consoleText(context, en, zh);
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(controller.refreshDevices());
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if ([
      AppLifecycleState.inactive,
      AppLifecycleState.paused,
      AppLifecycleState.detached,
      AppLifecycleState.hidden,
    ].contains(state)) {
      controller.disconnect();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (widget.controller == null) {
      controller.dispose();
    } else {
      controller.disconnect();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: controller,
    builder: (context, _) {
      final state = controller.status;
      final app = controller.selectedApp;
      final order =
          controller.draftOrder ??
          app?.actions.map((a) => a.id).toList() ??
          <String>[];
      final actions = {
        for (final a in app?.actions ?? <ConsoleAction>[]) a.id: a,
      };
      return Scaffold(
        appBar: AppBar(
          title: Text(t('App controls', 'App 快捷操作')),
          actions: [
            if (controller.connected)
              IconButton(
                tooltip: t('Disconnect', '断开连接'),
                onPressed: controller.disconnect,
                icon: const Icon(Icons.link_off),
              ),
          ],
        ),
        body: SafeArea(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 650),
              child: ListView(
                padding: const EdgeInsets.all(20),
                children: [
                  Text(
                    t('Your Mac, one tap away.', '点一下，操控你的 Mac。'),
                    style: Theme.of(context).textTheme.headlineSmall,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    t(
                      'Switch apps to see their shortcuts and recorded key sequences. Configure actions on Mac.',
                      '切换 App，显示它的快捷键和录制的键盘序列。在 Mac 上配置操作。',
                    ),
                  ),
                  const SizedBox(height: 20),
                  if (controller.simulation) ...[
                    Card(
                      color: Theme.of(context).colorScheme.tertiaryContainer,
                      child: Padding(
                        padding: const EdgeInsets.all(16),
                        child: Text(
                          t(
                            'BLUETOOTH SIMULATION · No real Bluetooth connection or system keys. Validate app switching, delays, stop and layout here.',
                            '蓝牙仿真 · 不连接真实蓝牙、不发送系统按键。可验证 App 切换、等待、停止和按钮排序。',
                          ),
                          style: Theme.of(context).textTheme.titleSmall,
                        ),
                      ),
                    ),
                    const SizedBox(height: 12),
                  ],
                  if (!controller.connected)
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(20),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            const Icon(Icons.bluetooth_rounded, size: 48),
                            const SizedBox(height: 16),
                            Text(
                              t('Connect a paired Mac', '连接已配对的 Mac'),
                              style: Theme.of(context).textTheme.titleLarge,
                            ),
                            const SizedBox(height: 8),
                            Text(
                              t(
                                'Enable Bluetooth control in Repose on Mac, then choose your paired Mac below.',
                                '在 Mac 的 Repose 中开启蓝牙控制，然后选择已配对的 Mac。',
                              ),
                            ),
                            const SizedBox(height: 16),
                            if (controller.devicesLoading)
                              const LinearProgressIndicator(),
                            if (controller.busy) ...[
                              const LinearProgressIndicator(),
                              Text(
                                t('Connecting over Bluetooth…', '正在通过蓝牙连接…'),
                              ),
                              const SizedBox(height: 12),
                            ],
                            for (final device in controller.devices)
                              Padding(
                                padding: const EdgeInsets.only(bottom: 8),
                                child: FilledButton.icon(
                                  onPressed: controller.busy
                                      ? null
                                      : () => controller.connect(device.id),
                                  icon: const Icon(Icons.bluetooth_connected),
                                  label: Text(device.name),
                                ),
                              ),
                            if (!controller.devicesLoading &&
                                controller.devices.isEmpty) ...[
                              Text(
                                t(
                                  'No paired Mac yet. Complete confirmation on both devices in Phone Key first.',
                                  '还没有已配对的 Mac。请先在「手机钥匙」完成两端配对确认。',
                                ),
                              ),
                              const SizedBox(height: 12),
                              OutlinedButton.icon(
                                onPressed: () =>
                                    Navigator.of(context).maybePop(),
                                icon: const Icon(Icons.key),
                                label: Text(
                                  t('Go to Phone Key pairing', '前往手机钥匙配对'),
                                ),
                              ),
                            ],
                            TextButton.icon(
                              onPressed:
                                  controller.devicesLoading || controller.busy
                                  ? null
                                  : controller.refreshDevices,
                              icon: const Icon(Icons.refresh),
                              label: Text(
                                t('Refresh paired devices', '刷新已配对设备'),
                              ),
                            ),
                            if (controller.devicesError != null)
                              Text(
                                controller.devicesError!,
                                style: TextStyle(
                                  color: Theme.of(context).colorScheme.error,
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  if (state != null && controller.connected) ...[
                    Row(
                      children: [
                        Icon(
                          state.blocked
                              ? Icons.pause_circle_outline
                              : Icons.check_circle_outline,
                          size: 18,
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            state.blocked
                                ? t(
                                    'Paused while Mac is locked or on a break',
                                    'Mac 已锁定或正在休息，控制已暂停',
                                  )
                                : !state.accessibility
                                ? t(
                                    'Allow Accessibility in Mac settings',
                                    '请在 Mac 设置中授予辅助功能权限',
                                  )
                                : state.running
                                ? t('Running keyboard sequence…', '正在执行键盘序列…')
                                : t('Connected to Mac', '已连接 Mac'),
                          ),
                        ),
                      ],
                    ),
                    if (state.running)
                      OutlinedButton.icon(
                        onPressed: controller.busy ? null : controller.cancel,
                        icon: const Icon(Icons.stop_circle_outlined),
                        label: Text(t('Stop sequence', '停止序列')),
                      ),
                    const SizedBox(height: 18),
                    SingleChildScrollView(
                      scrollDirection: Axis.horizontal,
                      child: Row(
                        children: [
                          for (final item in state.apps)
                            Padding(
                              padding: const EdgeInsets.only(right: 8),
                              child: ChoiceChip(
                                label: Text(item.name),
                                selected: app?.id == item.id,
                                onSelected: controller.canControl
                                    ? (_) => controller.activate(item.id)
                                    : null,
                              ),
                            ),
                        ],
                      ),
                    ),
                    const SizedBox(height: 16),
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            app?.name ?? t('Choose an app', '选择 App'),
                            style: Theme.of(context).textTheme.titleLarge,
                          ),
                        ),
                        if (!controller.arranging)
                          TextButton.icon(
                            onPressed: controller.canControl
                                ? controller.beginArrange
                                : null,
                            icon: const Icon(Icons.drag_indicator),
                            label: Text(t('Arrange', '调整位置')),
                          ),
                      ],
                    ),
                    if (controller.arranging) ...[
                      Text(
                        t(
                          'Hold and drag any button, or use its arrows. Actions are paused while arranging.',
                          '长按拖动任意按钮，也可用箭头调整。调整时不会执行操作。',
                        ),
                      ),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          TextButton(
                            onPressed: controller.busy
                                ? null
                                : controller.cancelArrange,
                            child: Text(t('Cancel', '取消')),
                          ),
                          FilledButton(
                            style: FilledButton.styleFrom(
                              minimumSize: const Size(48, 52),
                            ),
                            onPressed: controller.busy
                                ? null
                                : controller.saveOrder,
                            child: Text(t('Save layout', '保存布局')),
                          ),
                        ],
                      ),
                    ],
                    const SizedBox(height: 8),
                    GridView.builder(
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      itemCount: order.length,
                      gridDelegate:
                          const SliverGridDelegateWithFixedCrossAxisCount(
                            crossAxisCount: 2,
                            mainAxisExtent: 166,
                            crossAxisSpacing: 12,
                            mainAxisSpacing: 12,
                          ),
                      itemBuilder: (context, index) {
                        final action = actions[order[index]];
                        if (action == null) return const SizedBox.shrink();
                        final tile = _ActionTile(
                          action: action,
                          enabled: controller.canControl,
                          arranging: controller.arranging,
                          onTap: () => controller.execute(action.id),
                          onEarlier: index > 0
                              ? () => controller.move(action.id, index - 1)
                              : null,
                          onLater: index < order.length - 1
                              ? () => controller.move(action.id, index + 1)
                              : null,
                        );
                        if (!controller.arranging) return tile;
                        return DragTarget<String>(
                          onWillAcceptWithDetails: (details) =>
                              details.data != action.id,
                          onAcceptWithDetails: (details) =>
                              controller.move(details.data, index),
                          builder: (context, candidates, _) => DecoratedBox(
                            decoration: BoxDecoration(
                              border: candidates.isEmpty
                                  ? null
                                  : Border.all(
                                      color: Theme.of(
                                        context,
                                      ).colorScheme.primary,
                                      width: 3,
                                    ),
                              borderRadius: BorderRadius.circular(20),
                            ),
                            child: LongPressDraggable<String>(
                              data: action.id,
                              feedback: Material(
                                borderRadius: BorderRadius.circular(20),
                                elevation: 8,
                                child: SizedBox(
                                  width: 160,
                                  height: 166,
                                  child: tile,
                                ),
                              ),
                              childWhenDragging: Opacity(
                                opacity: 0.25,
                                child: tile,
                              ),
                              child: tile,
                            ),
                          ),
                        );
                      },
                    ),
                    if (order.isEmpty)
                      Padding(
                        padding: const EdgeInsets.all(20),
                        child: Text(
                          t(
                            'Add actions in Repose on Mac.',
                            '在 Mac 的 Repose 中添加操作。',
                          ),
                        ),
                      ),
                  ],
                  if (controller.error != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 16),
                      child: Text(
                        controller.connected
                            ? t(
                                'Request rejected. Check Mac status or cancel layout editing and try again.',
                                '请求未被接受。请检查 Mac 状态，或取消布局编辑后重试。',
                              )
                            : t(
                                'Bluetooth unavailable. Check Mac and reconnect from paired devices. Commands are never resent.',
                                '蓝牙连接不可用。请检查 Mac 后选择已配对设备重新连接。操作不会自动重发。',
                              ),
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.error,
                        ),
                      ),
                    ),
                  if (controller.error != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 8),
                      child: Text(
                        controller.error!,
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ),
                  if (state?.lastError != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 12),
                      child: Text(
                        t(
                          'An action failed. Check Repose on Mac for details.',
                          '操作失败，请在 Mac 的 Repose 中查看详情。',
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      );
    },
  );
}

class _ActionTile extends StatelessWidget {
  const _ActionTile({
    required this.action,
    required this.enabled,
    required this.arranging,
    required this.onTap,
    this.onEarlier,
    this.onLater,
  });
  final ConsoleAction action;
  final bool enabled, arranging;
  final VoidCallback onTap;
  final VoidCallback? onEarlier, onLater;
  @override
  Widget build(BuildContext context) => Card(
    margin: EdgeInsets.zero,
    color: action.kind == 'sequence'
        ? Theme.of(context).colorScheme.primaryContainer
        : Theme.of(context).colorScheme.surfaceContainerHighest,
    child: InkWell(
      key: ValueKey('console-action-${action.id}'),
      borderRadius: BorderRadius.circular(16),
      onTap: enabled ? onTap : null,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(
              action.kind == 'sequence'
                  ? Icons.playlist_play_rounded
                  : Icons.keyboard_command_key,
              size: 25,
            ),
            const SizedBox(height: 8),
            Text(
              action.name,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleSmall,
            ),
            const Spacer(),
            if (arranging)
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    tooltip: consoleText(context, 'Move earlier', '向前移动'),
                    onPressed: onEarlier,
                    icon: const Icon(Icons.arrow_back, size: 18),
                  ),
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    tooltip: consoleText(context, 'Move later', '向后移动'),
                    onPressed: onLater,
                    icon: const Icon(Icons.arrow_forward, size: 18),
                  ),
                ],
              )
            else
              Text(
                action.kind == 'sequence'
                    ? consoleText(
                        context,
                        '${action.stepCount} steps',
                        '${action.stepCount} 步序列',
                      )
                    : consoleText(context, 'Shortcut', '快捷键'),
                style: Theme.of(context).textTheme.labelSmall,
              ),
          ],
        ),
      ),
    ),
  );
}
