import 'package:flutter/services.dart';

enum PermissionAccess {
  notRequired,
  granted,
  denied,
  permanentlyDenied,
  unknown,
}

enum BackgroundAccess { optimized, restricted, unrestricted, unknown }

class SystemSettingsSnapshot {
  const SystemSettingsSnapshot({
    required this.platform,
    required this.nearby,
    required this.notifications,
    required this.background,
    this.manufacturer = '',
    this.powerSaver = false,
  });
  final String platform;
  final String manufacturer;
  final PermissionAccess nearby;
  final PermissionAccess notifications;
  final BackgroundAccess background;
  final bool powerSaver;

  factory SystemSettingsSnapshot.fromMap(Map<Object?, Object?> map) {
    final platform = map['platform'];
    if (platform != 'android' && platform != 'ios') {
      throw const FormatException('Unknown platform');
    }
    PermissionAccess permission(Object? value) =>
        PermissionAccess.values.firstWhere(
          (v) => v.name == value,
          orElse: () => PermissionAccess.unknown,
        );
    return SystemSettingsSnapshot(
      platform: platform as String,
      manufacturer: map['manufacturer'] is String
          ? map['manufacturer'] as String
          : '',
      nearby: permission(map['nearby']),
      notifications: permission(map['notifications']),
      background: BackgroundAccess.values.firstWhere(
        (v) => v.name == map['background'],
        orElse: () => BackgroundAccess.unknown,
      ),
      powerSaver: map['powerSaver'] == true,
    );
  }
}

class SystemSettingsGateway {
  static const _channel = MethodChannel('ai.repose/system_settings');
  Future<SystemSettingsSnapshot> read() async {
    final value = await _channel
        .invokeMapMethod<Object?, Object?>('getStatus')
        .timeout(const Duration(seconds: 8));
    if (value == null) throw const FormatException('Missing settings');
    return SystemSettingsSnapshot.fromMap(value);
  }

  Future<void> perform(String action) async {
    if (!const {
      'openAppSettings',
      'requestNearby',
      'requestNotifications',
    }.contains(action)) {
      throw ArgumentError.value(action);
    }
    final result = await _channel.invokeMethod<bool>(action);
    if (result != true) throw PlatformException(code: 'settingsUnavailable');
  }
}
