import 'dart:async';
import 'dart:convert';
import 'dart:math';
import 'package:flutter/services.dart';
import 'console_models.dart';

abstract interface class ConsoleTransport {
  Future<ConsoleStatus> request(Map<String, dynamic> message);
  void close();
}

class ConsoleDevice {
  const ConsoleDevice({required this.id, required this.name});
  final String id, name;
}

/// Native owns the confirmed Bluetooth associations, secret keys and GATT link.
/// Dart only sends bounded domain messages; it never gets a pairing secret.
class ConsoleClient implements ConsoleTransport {
  ConsoleClient(this.deviceId, {MethodChannel channel = bluetoothChannel})
    : _channel = channel {
    if (deviceId.isEmpty) throw ArgumentError('Choose a paired Mac');
    _owners[_channel.name] = _owner;
  }
  static const bluetoothChannel = MethodChannel('ai.repose/work_console_ble');
  static final Map<String, Object> _owners = {};
  final String deviceId;
  final MethodChannel _channel;
  final Object _owner = Object();
  bool _closed = false, _busy = false, _ready = false;
  Map<String, dynamic>? _config;

  static Future<List<ConsoleDevice>> devices({
    MethodChannel channel = bluetoothChannel,
  }) async {
    final values = await channel.invokeListMethod<dynamic>('devices');
    if (values == null || values.length > 64) {
      throw const FormatException('Invalid paired Bluetooth devices');
    }
    final result = <ConsoleDevice>[];
    final ids = <String>{};
    for (final value in values) {
      if (value is! Map || value['id'] is! String || value['name'] is! String) {
        throw const FormatException('Invalid paired Bluetooth device');
      }
      final id = value['id'] as String;
      final name = value['name'] as String;
      if (id.isEmpty ||
          id.length > 256 ||
          name.isEmpty ||
          name.length > 256 ||
          !ids.add(id)) {
        throw const FormatException('Invalid paired Bluetooth device');
      }
      result.add(ConsoleDevice(id: id, name: name));
    }
    return result;
  }

  void _assertOpen() {
    if (_closed || _owners[_channel.name] != _owner) {
      throw const ConsoleDisconnected();
    }
  }

  @override
  Future<ConsoleStatus> request(Map<String, dynamic> message) async {
    _assertOpen();
    if (_busy) throw StateError('A Bluetooth request is already pending');
    _busy = true;
    try {
      return await _send(message).timeout(const Duration(seconds: 30));
    } on ConsoleRequestRejected {
      rethrow;
    } catch (_) {
      close(); // No retry: the Mac may already have accepted an action.
      rethrow;
    } finally {
      _busy = false;
    }
  }

  Future<ConsoleStatus> _send(Map<String, dynamic> message) async {
    if (!_ready) {
      await _channel.invokeMethod<void>('connect', {'deviceId': deviceId});
      _assertOpen();
      _ready = true;
    }
    final payload = <String, dynamic>{...message};
    if (_config != null) payload['knownRevision'] = _config!['revision'];
    final raw = await _channel.invokeMethod<String>('request', {
      'message': jsonEncode(payload),
    });
    _assertOpen(); // A late reply must never recreate a disconnected session.
    if (raw == null || utf8.encode(raw).length > 262144) {
      throw const FormatException('Invalid Bluetooth console response');
    }
    final envelope = jsonDecode(raw);
    if (envelope is! Map<String, dynamic>) {
      throw const FormatException('Invalid Bluetooth console response');
    }
    if (envelope['ok'] == false) {
      throw ConsoleRequestRejected(
        envelope['error'] is String ? envelope['error'] as String : null,
      );
    }
    if (envelope['ok'] != true || envelope['data'] is! Map<String, dynamic>) {
      throw const FormatException('Invalid Bluetooth console response');
    }
    final data = Map<String, dynamic>.of(
      envelope['data'] as Map<String, dynamic>,
    );
    final config = data['config'] ?? _config;
    if (config is! Map<String, dynamic>) {
      throw const FormatException('First Bluetooth status must include config');
    }
    data['config'] = config;
    final status = ConsoleStatus.fromJson(data);
    _config = config;
    return status;
  }

  @override
  void close() {
    if (_closed) return;
    _closed = true;
    _config = null;
    if (_owners[_channel.name] == _owner) {
      _owners.remove(_channel.name);
      unawaited(
        _channel.invokeMethod<void>('disconnect').catchError((Object _) {}),
      );
    }
  }
}

class ConsoleDisconnected implements Exception {
  const ConsoleDisconnected();
  @override
  String toString() => 'Bluetooth disconnected';
}

class ConsoleRequestRejected implements Exception {
  ConsoleRequestRejected([this.message]);
  final String? message;
}

String consoleRequestId() {
  final random = Random.secure();
  return base64UrlEncode(
    List<int>.generate(18, (_) => random.nextInt(256)),
  ).replaceAll('=', '');
}
