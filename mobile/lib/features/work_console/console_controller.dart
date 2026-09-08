import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'console_client.dart';
import 'console_models.dart';
import 'console_simulator.dart';

typedef ConsoleTransportFactory = ConsoleTransport Function(String deviceId);
typedef ConsoleDevicesLoader = Future<List<ConsoleDevice>> Function();

class ConsoleController extends ChangeNotifier {
  ConsoleController({
    ConsoleTransportFactory? transportFactory,
    ConsoleDevicesLoader? devicesLoader,
    this.simulation = consoleSimulationEnabled,
  }) : _factory =
           transportFactory ??
           (simulation ? SimulatedConsoleClient.new : ConsoleClient.new),
       _devicesLoader =
           devicesLoader ??
           (simulation
               ? SimulatedConsoleClient.devices
               : ConsoleClient.devices);
  final ConsoleTransportFactory _factory;
  final ConsoleDevicesLoader _devicesLoader;
  final bool simulation;
  List<ConsoleDevice> devices = [];
  bool devicesLoading = false;
  String? devicesError;

  Future<void> refreshDevices() async {
    if (_disposed || devicesLoading) return;
    devicesLoading = true;
    devicesError = null;
    _notify();
    try {
      final next = await _devicesLoader();
      if (!_disposed) devices = next;
    } catch (reason) {
      if (!_disposed) {
        devices = [];
        devicesError = reason is PlatformException
            ? reason.message ?? reason.code
            : 'Bluetooth device list unavailable';
      }
    } finally {
      devicesLoading = false;
      _notify();
    }
  }

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

  Future<void> connect(String deviceId) async {
    if (_disposed) return;
    disconnect();
    error = null;
    final generation = _generation;
    try {
      _transport = _factory(deviceId);
      await _request({'type': 'status'});
      if (!_disposed && generation == _generation && connected) {
        _heartbeat = Timer.periodic(const Duration(seconds: 1), (_) {
          if (!busy) unawaited(_request({'type': 'status'}));
        });
      }
    } catch (_) {
      disconnect();
      error = 'Bluetooth connection unavailable';
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
    } on ConsoleRequestRejected catch (reason) {
      if (!_disposed && generation == _generation) {
        error =
            reason.message ??
            'Request rejected. Check Mac status or refresh the layout.';
      }
      return false;
    } catch (reason) {
      if (!_disposed && generation == _generation) {
        disconnect();
        error = reason is PlatformException
            ? reason.message ?? reason.code
            : 'Bluetooth disconnected. Choose the paired Mac to reconnect.';
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
