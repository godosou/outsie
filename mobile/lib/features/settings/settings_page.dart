import 'dart:async';
import 'package:flutter/material.dart';
import '../../app/app_text.dart';
import '../../app/repose_theme.dart';
import 'system_settings.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key});
  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage>
    with WidgetsBindingObserver {
  final _gateway = SystemSettingsGateway();
  SystemSettingsSnapshot? _status;
  bool _loading = true;
  bool _failed = false;
  bool _busy = false;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(_refresh());
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _generation++;
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) unawaited(_refresh());
  }

  Future<void> _refresh() async {
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _failed = false;
      _status = null;
    });
    try {
      final result = await _gateway.read();
      if (mounted && generation == _generation) {
        setState(() => _status = result);
      }
    } catch (_) {
      if (mounted && generation == _generation) setState(() => _failed = true);
    } finally {
      if (mounted && generation == _generation) {
        setState(() => _loading = false);
      }
    }
  }

  Future<void> _perform(String action) async {
    if (_busy) return;
    final ios = _status?.platform == 'ios';
    setState(() => _busy = true);
    try {
      await _gateway.perform(action);
      if (mounted && action != 'openAppSettings') await _refresh();
    } catch (_) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: AppText(
              ios
                  ? 'Open Settings → Repose manually.'
                  : 'Open Settings → Apps → Repose → Permissions or Battery manually.',
            ),
            duration: const Duration(seconds: 8),
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _explain({
    required String title,
    required String body,
    required String action,
    bool background = false,
  }) async {
    final accepted = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        scrollable: true,
        icon: Icon(
          background ? Icons.battery_saver_outlined : Icons.shield_outlined,
        ),
        title: AppText(title),
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            AppText(body),
            if (background) ...[
              const SizedBox(height: 16),
              const AppText(
                'This version cannot unlock your Mac automatically, even with background activity allowed.',
              ),
            ],
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const AppText('Not now'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: AppText(background ? 'Open app settings' : 'Continue'),
          ),
        ],
      ),
    );
    if (accepted == true && mounted) await _perform(action);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final status = _status;
    return Scaffold(
      appBar: AppBar(
        title: const AppText(
          'Permissions & background',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: _refresh,
          child: SingleChildScrollView(
            physics: const AlwaysScrollableScrollPhysics(),
            padding: const EdgeInsets.fromLTRB(20, 8, 20, 32),
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 680),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    AppText(
                      'Make yourself at home.',
                      style: theme.textTheme.headlineSmall?.copyWith(
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 12),
                    AppText(
                      'Permissions are explained before any system prompt. You can decide later in Settings.',
                      style: theme.textTheme.bodyMedium?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                        height: 1.6,
                      ),
                    ),
                    const SizedBox(height: 22),
                    if (_loading)
                      const Center(
                        child: Padding(
                          padding: EdgeInsets.all(32),
                          child: CircularProgressIndicator(),
                        ),
                      ),
                    if (_failed)
                      _SettingsCard(
                        icon: Icons.info_outline,
                        title: 'Could not check settings',
                        children: [
                          const AppText(
                            'Check your settings again, or use your Mac password for now.',
                          ),
                          TextButton(
                            onPressed: _refresh,
                            child: const AppText('Try again'),
                          ),
                        ],
                      ),
                    if (status != null) ...[
                      _permission(status.nearby, nearby: true),
                      const SizedBox(height: 12),
                      _permission(status.notifications, nearby: false),
                      const SizedBox(height: 12),
                      _SettingsCard(
                        icon: Icons.battery_saver_outlined,
                        title: 'Background activity',
                        children: [
                          if (status.platform == 'ios')
                            const AppText(
                              'iOS manages background activity. Automatic presence unlock is not available in this version; changing Background App Refresh will not enable it.',
                            )
                          else ...[
                            Semantics(
                              liveRegion: true,
                              child: AppText(switch (status.background) {
                                BackgroundAccess.optimized =>
                                  'Battery optimization is on',
                                BackgroundAccess.restricted =>
                                  'Background activity is restricted',
                                BackgroundAccess.unrestricted =>
                                  'No Android battery restriction detected',
                                BackgroundAccess.unknown => 'Unable to check',
                              }, style: theme.textTheme.titleSmall),
                            ),
                            const SizedBox(height: 8),
                            AppText(switch (status.background) {
                              BackgroundAccess.optimized =>
                                'Battery optimization may delay background work. It does not mean the app is blocked.',
                              BackgroundAccess.restricted =>
                                'Android may stop this app in the background. Review its battery settings if you use phone key.',
                              _ =>
                                'Your phone may still apply its own background rules. This is not a guarantee of continuous operation.',
                            }),
                            if ([
                              'realme',
                              'oppo',
                              'oneplus',
                            ].contains(status.manufacturer.toLowerCase())) ...[
                              const SizedBox(height: 12),
                              const AppText(
                                'On realme / OPPO, look for App battery management, Allow background activity, or Auto launch. Names vary by system version.',
                              ),
                            ],
                            const SizedBox(height: 14),
                            OutlinedButton.icon(
                              key: const Key('backgroundSettingsButton'),
                              onPressed: _busy
                                  ? null
                                  : () => _explain(
                                      title: 'Before opening Settings',
                                      body:
                                          'Allowing background activity may use more battery. You can change this later.',
                                      action: 'openAppSettings',
                                      background: true,
                                    ),
                              icon: const Icon(Icons.open_in_new, size: 18),
                              label: const AppText(
                                'Review background settings',
                              ),
                            ),
                          ],
                          if (status.powerSaver) ...[
                            const Divider(),
                            AppText(
                              'Power saving is on',
                              style: theme.textTheme.titleSmall,
                            ),
                            const SizedBox(height: 6),
                            const AppText(
                              'Power saving can delay background activity. You can review it in system settings.',
                            ),
                          ],
                        ],
                      ),
                    ],
                    const SizedBox(height: 16),
                    _SettingsCard(
                      icon: Icons.lock_outline,
                      title: 'Your key, your choice.',
                      children: [
                        const AppText(
                          'Pair only your own devices. You can remove a key at any time. Your Mac password remains available.',
                        ),
                      ],
                    ),
                    const SizedBox(height: 32),
                    const Center(child: ReposeBrandMark()),
                    const SizedBox(height: 10),
                    AppText(
                      'Give your day a little space.',
                      textAlign: TextAlign.center,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _permission(PermissionAccess access, {required bool nearby}) {
    final theme = Theme.of(context);
    final notRequired = access == PermissionAccess.notRequired;
    final denied = access == PermissionAccess.denied;
    final blocked = access == PermissionAccess.permanentlyDenied;
    return _SettingsCard(
      icon: nearby ? Icons.bluetooth : Icons.notifications_none,
      title: nearby ? 'Nearby devices' : 'Notifications',
      children: [
        Semantics(
          liveRegion: true,
          child: AppText(
            switch (access) {
              PermissionAccess.notRequired => 'Not requested in this version',
              PermissionAccess.granted => 'Allowed',
              PermissionAccess.denied => 'Permission needed',
              PermissionAccess.permanentlyDenied => 'Allow in Settings',
              PermissionAccess.unknown => 'Unable to check',
            },
            style: theme.textTheme.titleSmall?.copyWith(
              color: theme.colorScheme.primary,
            ),
          ),
        ),
        const SizedBox(height: 8),
        AppText(
          nearby
              ? (notRequired
                    ? 'Bluetooth access is used to communicate with your paired Mac. This version does not request it yet.'
                    : 'Allow Bluetooth access to communicate with your paired Mac.')
              : (notRequired
                    ? 'Notifications will explain connection issues when supported. This version does not send them.'
                    : 'Optional reminders about phone-key connection issues.'),
        ),
        if (denied || blocked) ...[
          const SizedBox(height: 14),
          FilledButton(
            key: Key(
              nearby
                  ? 'nearbyPermissionButton'
                  : 'notificationPermissionButton',
            ),
            onPressed: _busy
                ? null
                : () => blocked
                      ? _perform('openAppSettings')
                      : _explain(
                          title: nearby
                              ? 'Allow nearby devices?'
                              : 'Allow notifications?',
                          body: nearby
                              ? 'Repose uses Bluetooth to communicate with your paired Mac. You can decline and keep using your Mac password.'
                              : 'Receive reminders if your phone key needs attention. This is optional.',
                          action: nearby
                              ? 'requestNearby'
                              : 'requestNotifications',
                        ),
            child: AppText(blocked ? 'Open settings' : 'Allow'),
          ),
        ],
      ],
    );
  }
}

class _SettingsCard extends StatelessWidget {
  const _SettingsCard({
    required this.icon,
    required this.title,
    required this.children,
  });
  final IconData icon;
  final String title;
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => Card(
    child: Padding(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(
                icon,
                size: 22,
                color: Theme.of(context).colorScheme.primary,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: AppText(
                  title,
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          ...children,
        ],
      ),
    ),
  );
}
