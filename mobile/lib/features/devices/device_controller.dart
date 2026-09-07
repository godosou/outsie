import 'package:flutter/foundation.dart';

import '../../native/native_gateway.dart';
import '../../native/native_models.dart';

class CapabilityGate {
  const CapabilityGate._({
    required this.canPair,
    required this.canCalibrate,
    this.message,
    this.isPermanentlyUnsupported = false,
  });

  factory CapabilityGate.from(CompanionCapability capability) {
    return switch (capability) {
      CompanionCapability.ready => const CapabilityGate._(
        canPair: true,
        canCalibrate: true,
      ),
      CompanionCapability.loading => const CapabilityGate._(
        canPair: false,
        canCalibrate: false,
        message: 'Checking native phone-key capabilities…',
      ),
      CompanionCapability.bluetoothUnavailable => const CapabilityGate._(
        canPair: false,
        canCalibrate: false,
        message: 'Bluetooth is unavailable. Turn it on to continue.',
      ),
      CompanionCapability.secureHardwareUnavailable => const CapabilityGate._(
        canPair: false,
        canCalibrate: false,
        message: 'Secure hardware keys are unavailable on this device.',
      ),
      CompanionCapability.backgroundExecutionUnavailable =>
        const CapabilityGate._(
          canPair: false,
          canCalibrate: false,
          message: 'Allow background execution before enabling phone key.',
        ),
      CompanionCapability.unsupportedPlatform => const CapabilityGate._(
        canPair: false,
        canCalibrate: false,
        isPermanentlyUnsupported: true,
        message: 'Automatic presence unlock is unsupported on this platform.',
      ),
      CompanionCapability.nativeBridgeUnavailable => const CapabilityGate._(
        canPair: false,
        canCalibrate: false,
        message: 'The native phone-key service is not connected yet.',
      ),
    };
  }

  static const refreshing = CapabilityGate._(
    canPair: false,
    canCalibrate: false,
    message: 'Refreshing authoritative phone-key state…',
  );

  static const mutating = CapabilityGate._(
    canPair: false,
    canCalibrate: false,
    message: 'A phone-key update is in progress…',
  );

  final bool canPair;
  final bool canCalibrate;
  final bool isPermanentlyUnsupported;
  final String? message;
}

class DeviceState {
  DeviceState({
    this.snapshot,
    this.isLoading = false,
    this.isFailClosed = false,
    this.message,
    Iterable<String> revokingDeviceIds = const <String>[],
  }) : revokingDeviceIds = Set<String>.unmodifiable(revokingDeviceIds);

  final UnlockSnapshot? snapshot;
  final bool isLoading;
  final bool isFailClosed;
  final String? message;
  final Set<String> revokingDeviceIds;

  bool get isHydrated => snapshot != null && !isLoading && !isFailClosed;
  bool get isMutationBusy => revokingDeviceIds.isNotEmpty;
  List<PairedDevice> get devices => snapshot?.devices ?? const <PairedDevice>[];
  CalibrationSnapshot get calibration =>
      snapshot?.calibration ?? const CalibrationSnapshot();

  CapabilityGate get gate {
    if (isFailClosed) {
      return CapabilityGate.from(CompanionCapability.nativeBridgeUnavailable);
    }
    if (isLoading) {
      return CapabilityGate.refreshing;
    }
    if (isMutationBusy) {
      return CapabilityGate.mutating;
    }
    return CapabilityGate.from(
      snapshot?.capability ?? CompanionCapability.loading,
    );
  }
}

class DeviceController extends ChangeNotifier {
  DeviceController({required NativeGateway gateway}) : _gateway = gateway;

  final NativeGateway _gateway;
  DeviceState _state = DeviceState();
  var _epoch = 0;
  var _disposed = false;
  var _revokeInFlight = false;

  DeviceState get state => _state;

  Future<bool> hydrate() async {
    if (_disposed || _revokeInFlight) {
      return false;
    }
    final token = ++_epoch;
    final previous = _state.snapshot;
    _replace(DeviceState(snapshot: previous, isLoading: true), token: token);
    try {
      final snapshot = await _gateway.getSnapshot();
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(DeviceState(snapshot: snapshot), token: token);
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        DeviceState(
          snapshot: previous,
          isFailClosed: true,
          message:
              error.safeMessage ?? 'Native phone-key state is unavailable.',
        ),
        token: token,
      );
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        DeviceState(
          snapshot: previous,
          isFailClosed: true,
          message: 'Native phone-key state could not be refreshed.',
        ),
        token: token,
      );
      return false;
    }
  }

  Future<bool> revokeDevice(String deviceId) async {
    if (_disposed ||
        _revokeInFlight ||
        _state.isLoading ||
        !_state.devices.any((device) => device.id == deviceId)) {
      return false;
    }

    _revokeInFlight = true;
    final token = ++_epoch;
    final previous = _state.snapshot;
    _replace(
      DeviceState(snapshot: previous, revokingDeviceIds: <String>{deviceId}),
      token: token,
    );
    try {
      await _gateway.revokeDevice(deviceId);
      if (!_isCurrent(token)) {
        return false;
      }
      final refreshed = await _gateway.getSnapshot();
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        DeviceState(snapshot: refreshed, message: 'Phone key revoked.'),
        token: token,
      );
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        DeviceState(
          snapshot: previous,
          isFailClosed: true,
          message: error.safeMessage ?? 'Device revocation was not confirmed.',
        ),
        token: token,
      );
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        DeviceState(
          snapshot: previous,
          isFailClosed: true,
          message: 'Device revocation was not confirmed.',
        ),
        token: token,
      );
      return false;
    } finally {
      _revokeInFlight = false;
    }
  }

  bool _isCurrent(int token) => !_disposed && token == _epoch;

  void _replace(DeviceState next, {required int token}) {
    if (!_isCurrent(token)) {
      return;
    }
    _state = next;
    notifyListeners();
  }

  @override
  void dispose() {
    if (_disposed) {
      return;
    }
    _disposed = true;
    _epoch += 1;
    super.dispose();
  }
}
