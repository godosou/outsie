import 'dart:async';
import 'package:flutter/foundation.dart';
import 'console_client.dart';
import 'console_models.dart';

typedef ConsoleTransportFactory = ConsoleTransport Function(ConsolePairing);

class ConsoleController extends ChangeNotifier {
  ConsoleController({ConsoleTransportFactory? transportFactory})
    : _factory = transportFactory ?? ConsoleClient.new;
  final ConsoleTransportFactory _factory;
  ConsoleTransport? _transport;
  Timer? _heartbeat;
  bool _disposed = false;
  int _generation = 0;
  bool busy = false;
  ConsoleStatus? status;
  String? error;
  String? selectedAppId;
  List<String>? draftOrder;
  int? _draftRevision;
  bool get connected => _transport != null && status != null;
  bool get arranging => draftOrder != null;
  bool get canControl =>
      connected &&
      !busy &&
      !arranging &&
      status!.enabled &&
      !status!.running &&
      !status!.blocked &&
      status!.accessibility;
  ConsoleApp? get selectedApp {
    for (final app in status?.apps ?? <ConsoleApp>[]) {
      if (app.id == selectedAppId) return app;
    }
    return null;
  }

  Future<void> connect(String qr) async {
    disconnect();
    error = null;
    try {
      _transport = _factory(ConsolePairing.parse(qr));
      await _request({'type': 'status'});
      if (connected) {
        _heartbeat = Timer.periodic(const Duration(seconds: 1), (_) {
          if (!busy) unawaited(_request({'type': 'status'}));
        });
      }
    } catch (_) {
      disconnect();
      error = 'Invalid connection QR code';
      _notify();
    }
  }

  Future<bool> _request(Map<String, dynamic> message) async {
    final transport = _transport;
    if (transport == null || busy || _disposed) return false;
    busy = true;
    final generation = _generation;
    _notify();
    try {
      final next = await transport.request(message);
      if (_disposed || generation != _generation) return false;
      status = next;
      if (selectedAppId == null ||
          !next.apps.any((app) => app.id == selectedAppId)) {
        selectedAppId = next.activeAppId ?? next.apps.firstOrNull?.id;
      }
      if (message['type'] != 'status') error = null;
      return true;
    } on ConsoleRequestRejected {
      if (!_disposed && generation == _generation) {
        error = 'Request rejected. Check Mac status or refresh the layout.';
      }
      return false;
    } catch (_) {
      if (!_disposed && generation == _generation) {
        disconnect();
        error = 'Connection lost. Scan again to reconnect.';
      }
      return false;
    } finally {
      if (!_disposed && generation == _generation) busy = false;
      _notify();
    }
  }

  Future<void> activate(String appId) async {
    if (!canControl) return;
    if (await _request({
      'type': 'activate',
      'requestId': consoleRequestId(),
      'appId': appId,
    })) {
      selectedAppId = appId;
      _notify();
    }
  }

  Future<void> execute(String actionId) async {
    if (!canControl || selectedApp == null) return;
    await _request({
      'type': 'execute',
      'requestId': consoleRequestId(),
      'appId': selectedAppId,
      'actionId': actionId,
    });
  }

  Future<void> cancel() async {
    if (!connected || busy) return;
    await _request({'type': 'cancel', 'requestId': consoleRequestId()});
  }

  void beginArrange() {
    if (!canControl || selectedApp == null) return;
    draftOrder = selectedApp!.actions.map((a) => a.id).toList();
    _draftRevision = status!.revision;
    _notify();
  }

  void move(String actionId, int target) {
    final order = draftOrder;
    if (order == null || target < 0 || target >= order.length) return;
    if (!order.remove(actionId)) return;
    order.insert(target, actionId);
    _notify();
  }

  void cancelArrange() {
    draftOrder = null;
    _draftRevision = null;
    _notify();
  }

  Future<void> saveOrder() async {
    if (!connected || busy || draftOrder == null) return;
    if (await _request({
      'type': 'reorder',
      'requestId': consoleRequestId(),
      'appId': selectedAppId,
      'revision': _draftRevision,
      'actionIds': List<String>.of(draftOrder!),
    })) {
      cancelArrange();
    }
  }

  void disconnect() {
    _generation++;
    _heartbeat?.cancel();
    _heartbeat = null;
    _transport?.close();
    _transport = null;
    status = null;
    busy = false;
    selectedAppId = null;
    draftOrder = null;
    _draftRevision = null;
    _notify();
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    disconnect();
    super.dispose();
  }
}
