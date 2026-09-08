import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import '../pairing/pairing_scanner.dart';
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
  bool scanning = false;
  String t(String en, String zh) => consoleText(context, en, zh);
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
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

  Future<void> scan() async {
    if (scanning) return;
    setState(() => scanning = true);
    final access = await const SystemPairingCameraAccess().request();
    if (!mounted) return;
    if (access != CameraAccessOutcome.granted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            t('Allow camera access in Settings.', '请在系统设置中允许相机权限。'),
          ),
          action: SnackBarAction(
            label: t('Settings', '设置'),
            onPressed: () {
              const SystemPairingCameraAccess().openSettings();
            },
          ),
        ),
      );
    } else {
      final qr = await Navigator.of(context).push<String>(
        MaterialPageRoute(builder: (_) => const ConsoleScannerPage()),
      );
      if (mounted && qr != null) await controller.connect(qr);
    }
    if (mounted) setState(() => scanning = false);
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
                  if (!controller.connected)
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(20),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            const Icon(Icons.devices_rounded, size: 48),
                            const SizedBox(height: 16),
                            Text(
                              t('Connect to your Mac', '连接你的 Mac'),
                              style: Theme.of(context).textTheme.titleLarge,
                            ),
                            const SizedBox(height: 8),
                            Text(
                              t(
                                'Enable the local connection in Repose App controls on Mac, then scan its QR code. Both devices need the same Wi-Fi.',
                                '在 Mac 的 Repose「App 快捷操作」中开启局域网连接，然后扫码。两台设备需处于同一个 Wi-Fi。',
                              ),
                            ),
                            const SizedBox(height: 16),
                            FilledButton.icon(
                              onPressed: scanning || controller.busy
                                  ? null
                                  : scan,
                              icon: const Icon(Icons.qr_code_scanner),
                              label: Text(t('Scan Mac QR code', '扫描 Mac 二维码')),
                            ),
                            Text(
                              t(
                                'The QR code grants control. Credentials stay in memory and are removed on disconnect.',
                                '二维码用于授权控制。凭据仅保存在内存中，断开后即清除。',
                              ),
                              style: Theme.of(context).textTheme.bodySmall,
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
                                'Connection unavailable. Check Mac and scan again. Commands are never resent.',
                                '连接不可用。请检查 Mac 后重新扫码。操作不会自动重发。',
                              ),
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.error,
                        ),
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

class ConsoleScannerPage extends StatefulWidget {
  const ConsoleScannerPage({super.key});
  @override
  State<ConsoleScannerPage> createState() => _ConsoleScannerPageState();
}

class _ConsoleScannerPageState extends State<ConsoleScannerPage> {
  bool done = false, invalid = false;
  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: Text(consoleText(context, 'Scan Mac control QR', '扫描 Mac 控制二维码')),
    ),
    body: Stack(
      children: [
        MobileScanner(
          onDetect: (capture) {
            if (done) return;
            for (final barcode in capture.barcodes) {
              final raw = barcode.rawValue;
              if (raw == null) continue;
              try {
                ConsolePairing.parse(raw);
                done = true;
                Navigator.of(context).pop(raw);
                return;
              } catch (_) {
                if (!invalid) setState(() => invalid = true);
              }
            }
          },
          errorBuilder: (context, error) => Center(
            child: Text(
              consoleText(
                context,
                'Camera unavailable. Check camera permission in Settings.',
                '相机不可用，请在系统设置中检查相机权限。',
              ),
            ),
          ),
        ),
        if (invalid)
          Align(
            alignment: Alignment.bottomCenter,
            child: SafeArea(
              child: Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Text(
                    consoleText(
                      context,
                      'Scan the QR in Mac App controls, not the Phone Key code.',
                      '请扫描 Mac「App 快捷操作」中的二维码，而非手机钥匙配对码。',
                    ),
                  ),
                ),
              ),
            ),
          ),
      ],
    ),
  );
}
