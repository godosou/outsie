import 'package:flutter/foundation.dart';

import '../../native/native_gateway.dart';
import '../../native/native_models.dart';
import '../devices/device_controller.dart';

class CalibrationState {
  const CalibrationState({
    this.phase = CalibrationPhase.idle,
    this.message,
    this.isBusy = false,
  });

  final CalibrationPhase phase;
  final String? message;
  final bool isBusy;

  bool get canRetry => phase == CalibrationPhase.overlapRejected && !isBusy;
}

class CalibrationController extends ChangeNotifier {
  CalibrationController({
    required NativeGateway gateway,
    required DeviceController deviceController,
  }) : _gateway = gateway,
       _devices = deviceController;

  final NativeGateway _gateway;
  final DeviceController _devices;
  CalibrationState _state = const CalibrationState();
  var _epoch = 0;
  var _disposed = false;

  CalibrationState get state => _state;

  void hydrateFromSnapshot(CalibrationSnapshot snapshot) {
    if (_disposed) {
      return;
    }
    final token = ++_epoch;
    final message = switch (snapshot.phase) {
      CalibrationPhase.idle => null,
      CalibrationPhase.collectingNear =>
        'Hold the phone nearby for the near sample.',
      CalibrationPhase.collectingFar => 'Move away and collect the far sample.',
      CalibrationPhase.complete => 'Distance calibration complete.',
      CalibrationPhase.overlapRejected =>
        'Near and far samples overlap. Retry calibration.',
      CalibrationPhase.unavailable => 'Calibration is unavailable.',
      CalibrationPhase.failed => 'Calibration needs attention.',
    };
    _replace(
      CalibrationState(phase: snapshot.phase, message: message),
      token: token,
    );
  }

  Future<bool> startCalibration() async {
    if (_disposed || _state.isBusy) {
      return false;
    }
    if (!_isAllowed) {
      _setUnavailable();
      return false;
    }

    final token = ++_epoch;
    _replace(
      CalibrationState(
        phase: _state.phase,
        message: 'Starting calibration…',
        isBusy: true,
      ),
      token: token,
    );
    try {
      await _gateway.startCalibration();
      if (!_isCurrent(token)) {
        return false;
      }
      if (!_isAllowed) {
        _replace(_unavailableState(), token: token);
        return false;
      }
      _replace(
        const CalibrationState(
          phase: CalibrationPhase.collectingNear,
          message: 'Hold the phone nearby for the near sample.',
        ),
        token: token,
      );
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(_nativeFailure(error), token: token);
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        const CalibrationState(
          phase: CalibrationPhase.failed,
          message: 'Calibration could not be started.',
        ),
        token: token,
      );
      return false;
    }
  }

  Future<bool> submitStep(CalibrationStep step) async {
    if (_disposed || _state.isBusy) {
      return false;
    }
    if (!_isAllowed) {
      _setUnavailable();
      return false;
    }

    final expected = switch (_state.phase) {
      CalibrationPhase.collectingNear => CalibrationStep.near,
      CalibrationPhase.collectingFar => CalibrationStep.far,
      _ => null,
    };
    if (step != expected) {
      final instruction = expected == CalibrationStep.near
          ? 'Collect the near sample before the far sample.'
          : expected == CalibrationStep.far
          ? 'Collect the far sample after the near sample.'
          : 'Start calibration before submitting a sample.';
      final token = ++_epoch;
      _replace(
        CalibrationState(phase: _state.phase, message: instruction),
        token: token,
      );
      return false;
    }

    final token = ++_epoch;
    final activePhase = _state.phase;
    _replace(
      CalibrationState(
        phase: activePhase,
        message: _state.message,
        isBusy: true,
      ),
      token: token,
    );
    try {
      await _gateway.submitCalibrationStep(step);
      if (!_isCurrent(token)) {
        return false;
      }
      if (!_isAllowed) {
        _replace(_unavailableState(), token: token);
        return false;
      }
      if (step == CalibrationStep.near) {
        _replace(
          const CalibrationState(
            phase: CalibrationPhase.collectingFar,
            message: 'Move away and collect the far sample.',
          ),
          token: token,
        );
      } else {
        _replace(
          const CalibrationState(
            phase: CalibrationPhase.complete,
            message: 'Distance calibration complete.',
          ),
          token: token,
        );
      }
      return true;
    } on NativeGatewayException catch (error) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(_nativeFailure(error), token: token);
      return false;
    } catch (_) {
      if (!_isCurrent(token)) {
        return false;
      }
      _replace(
        const CalibrationState(
          phase: CalibrationPhase.failed,
          message: 'Calibration step failed.',
        ),
        token: token,
      );
      return false;
    }
  }

  bool get _isAllowed =>
      _devices.state.gate.canCalibrate && _devices.state.devices.isNotEmpty;

  CalibrationState _unavailableState() => const CalibrationState(
    phase: CalibrationPhase.unavailable,
    message: 'Calibration is unavailable until capability and pairing recover.',
  );

  void _setUnavailable() {
    if (_disposed) {
      return;
    }
    final token = ++_epoch;
    _replace(_unavailableState(), token: token);
  }

  CalibrationState _nativeFailure(NativeGatewayException error) {
    if (error.code == NativeErrorCode.calibrationOverlap) {
      return const CalibrationState(
        phase: CalibrationPhase.overlapRejected,
        message: 'Near and far samples overlap. Retry calibration.',
      );
    }
    if (error.code == NativeErrorCode.capabilityUnavailable ||
        error.code == NativeErrorCode.unsupported ||
        error.code == NativeErrorCode.bridgeUnavailable) {
      return CalibrationState(
        phase: CalibrationPhase.unavailable,
        message: error.safeMessage ?? 'Calibration is unavailable.',
      );
    }
    return CalibrationState(
      phase: CalibrationPhase.failed,
      message: error.safeMessage ?? 'Calibration step was rejected.',
    );
  }

  bool _isCurrent(int token) => !_disposed && token == _epoch;

  void _replace(CalibrationState next, {required int token}) {
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
