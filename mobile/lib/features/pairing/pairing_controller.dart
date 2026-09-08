import 'package:flutter/foundation.dart';

import '../../native/native_gateway.dart';
import '../../native/native_models.dart';
import '../devices/device_controller.dart';

typedef PairingClock = DateTime Function();

class PairingState {
  const PairingState({
    this.phase = PairingPhase.idle,
    this.session,
    this.message,
  });

  final PairingPhase phase;
  final PairingSession? session;
  final String? message;

  String? get deviceName => session?.deviceName;
  bool get isBusy =>
      phase == PairingPhase.starting || phase == PairingPhase.confirming;
}

class PairingController extends ChangeNotifier {
  PairingController({
    required NativeGateway gateway,
    required DeviceController deviceController,
    PairingClock? clock,
  }) : _gateway = gateway,
       _devices = deviceController,
       _clock = clock ?? DateTime.now;

  final NativeGateway _gateway;
  final DeviceController _devices;
  final PairingClock _clock;
  PairingState _state = const PairingState();
  var _epoch = 0;
  var _disposed = false;
  var _pendingReadInFlight = false;

  PairingState get state => _state;

  void hydrateFromSnapshot(PairingSession? session) {
    if (_disposed) {
      return;
    }
    final token = ++_epoch;
    if (session == null) {
      _replace(const PairingState(), token: token);
      return;
    }
    if (_isExpired(session)) {
      _replace(_expired(session), token: token);
      return;
    }
    _replace(
      PairingState(
        phase: PairingPhase.awaitingConfirmation,
        session: session,
        message: 'Confirm the device name before pairing.',
      ),
      token: token,
    );
  }

  /// Read pending status without putting DeviceController in its loading gate.
  /// Confirmation and new pairing increment the epoch, invalidating this read.
  Future<void> refreshPendingPairing() async {
    final session = _state.session;
    if (_disposed ||
        _pendingReadInFlight ||
        _state.phase != PairingPhase.awaitingConfirmation ||
        session == null) {
      return;
    }
    final token = _epoch;
    _pendingReadInFlight = true;
    try {
      final snapshot = await _gateway.getSnapshot();
      if (!_isCurrent(token) ||
          _state.phase != PairingPhase.awaitingConfirmation) {
        return;
      }
      if (_isExpired(session)) {
        _replace(_expired(session), token: token);
      } else if (snapshot.pendingPairing?.sessionId != session.sessionId) {
        _replace(
          const PairingState(
            phase: PairingPhase.failed,
            message: 'Pairing connection ended. Scan a new QR code.',
          ),
          token: token,
        );
      } else if (!CapabilityGate.from(snapshot.capability).canPair) {
        _replace(_unavailableState(session: session), token: token);
      }
    } catch (_) {
      if (_isCurrent(token)) {
        _replace(
          const PairingState(
            phase: PairingPhase.failed,
            message: 'Pairing connection ended. Scan a new QR code.',
          ),
          token: token,
        );
      }
    } finally {
      _pendingReadInFlight = false;
    }
  }

  Future<bool> beginPairing(String qrPayload) async {
    if (_disposed || _state.isBusy) {
      return false;
    }
    if (!_devices.state.gate.canPair) {
      _setUnavailable();
      return false;
    }
    if (qrPayload.isEmpty) {
      final token = ++_epoch;
      _replace(
        const PairingState(
          phase: PairingPhase.failed,
          message: 'Scan a valid pairing QR code first.',
        ),
        token: token,
      );
      return false;
    }

    final token = ++_epoch;
    _replace(const PairingState(phase: PairingPhase.starting), token: token);
    try {
      final session = await _gateway.beginPairing(qrPayload);
      if (!_isCurrent(token)) {
        return false;
      }
      if (!_devices.state.gate.canPair) {
        _replace(_unavailableState(), token: token);
        return false;
      }
      if (_isExpired(session)) {
        _replace(_expired(session), token: token);
        return false;
      }
      _replace(
        PairingState(
          phase: PairingPhase.awaitingConfirmation,
          session: session,
          message: 'Confirm the device name before pairing.',
        ),
        token: token,
      );
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(_pairingFailure(error), token: token);
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        const PairingState(
          phase: PairingPhase.failed,
          message: 'Pairing could not be started.',
        ),
        token: token,
      );
      return false;
    }
  }

  Future<bool> confirmPairing() async {
    if (_disposed || _state.isBusy) {
      return false;
    }
    final session = _state.session;
    if (session == null || _state.phase != PairingPhase.awaitingConfirmation) {
      return false;
    }
    if (!_devices.state.gate.canPair) {
      _setUnavailable(session: session);
      return false;
    }
    if (_isExpired(session)) {
      final token = ++_epoch;
      _replace(_expired(session), token: token);
      return false;
    }

    final priorDeviceIds = _devices.state.devices
        .map((device) => device.id)
        .toSet();
    final token = ++_epoch;
    _replace(
      PairingState(phase: PairingPhase.confirming, session: session),
      token: token,
    );
    try {
      await _gateway.confirmPairing(session.sessionId);
      if (!_isCurrent(token)) {
        return false;
      }

      final refreshed = await _devices.hydrate();
      if (!_isCurrent(token)) {
        return false;
      }
      final snapshot = _devices.state.snapshot;
      final authoritativePair =
          refreshed &&
          _devices.state.gate.canPair &&
          snapshot?.pendingPairing == null &&
          _devices.state.devices.any(
            (device) =>
                !priorDeviceIds.contains(device.id) &&
                device.displayName == session.deviceName,
          );
      if (!authoritativePair) {
        _replace(
          PairingState(
            phase: PairingPhase.failed,
            session: session,
            message:
                'Pairing confirmation could not refresh authoritative state.',
          ),
          token: token,
        );
        return false;
      }

      _replace(
        PairingState(phase: PairingPhase.paired, session: session),
        token: token,
      );
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(_pairingFailure(error, session: session), token: token);
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        PairingState(
          phase: PairingPhase.failed,
          session: session,
          message: 'Pairing confirmation failed.',
        ),
        token: token,
      );
      return false;
    }
  }

  bool _isExpired(PairingSession session) =>
      !_clock().isBefore(session.expiresAt);

  PairingState _expired(PairingSession session) => PairingState(
    phase: PairingPhase.expired,
    session: session,
    message: 'This pairing QR code has expired. Scan a new QR code.',
  );

  PairingState _unavailableState({PairingSession? session}) => PairingState(
    phase: PairingPhase.failed,
    session: session,
    message: 'Phone-key capability is unavailable.',
  );

  void _setUnavailable({PairingSession? session}) {
    if (_disposed) {
      return;
    }
    final token = ++_epoch;
    _replace(_unavailableState(session: session), token: token);
  }

  PairingState _pairingFailure(
    NativeGatewayException error, {
    PairingSession? session,
  }) {
    if (error.code == NativeErrorCode.qrExpired) {
      return PairingState(
        phase: PairingPhase.expired,
        session: session,
        message: 'This pairing QR code has expired. Scan a new QR code.',
      );
    }
    if (error.code == NativeErrorCode.qrAlreadyUsed) {
      return PairingState(
        phase: PairingPhase.failed,
        session: session,
        message: 'This pairing QR code was already used. Scan a new QR code.',
      );
    }
    if (error.code == NativeErrorCode.pairingNotAccepted && session != null) {
      return PairingState(
        phase: PairingPhase.awaitingConfirmation,
        session: session,
        message:
            error.safeMessage ??
            'Still connecting to the Repose Mac. Wait a moment and try again.',
      );
    }
    return PairingState(
      phase: PairingPhase.failed,
      session: session,
      message: error.safeMessage ?? 'Pairing is unavailable.',
    );
  }

  bool _isCurrent(int token) => !_disposed && token == _epoch;

  void _replace(PairingState next, {required int token}) {
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
