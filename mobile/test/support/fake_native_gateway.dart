import 'dart:async';

import 'package:repose_unlock/native/native_gateway.dart';
import 'package:repose_unlock/native/native_models.dart';

class FakeNativeGateway implements NativeGateway {
  FakeNativeGateway({UnlockSnapshot? snapshot, PairingSession? pairingSession})
    : snapshot = snapshot ?? UnlockSnapshot.empty(),
      pairingSession =
          pairingSession ??
          PairingSession(
            sessionId: 'session-1',
            deviceName: 'realme GT5 Pro',
            expiresAt: DateTime.utc(2026, 9, 8, 10, 2),
          );

  UnlockSnapshot snapshot;
  PairingSession pairingSession;
  Object? beginPairingError;
  Object? confirmPairingError;
  Object? snapshotError;
  Object? revokeError;
  FutureOr<PairingSession> Function(String qrPayload)? onBeginPairing;
  FutureOr<void> Function(String sessionId)? onConfirmPairing;
  FutureOr<UnlockSnapshot> Function()? onGetSnapshot;
  FutureOr<void> Function()? onStartCalibration;
  FutureOr<void> Function(CalibrationStep step)? onSubmitCalibration;
  final Map<CalibrationStep, Object> calibrationSubmissionErrors =
      <CalibrationStep, Object>{};
  FutureOr<void> Function(String deviceId)? onRevoke;
  final List<String> pairingPayloads = <String>[];
  final List<String> confirmedSessionIds = <String>[];
  final List<CalibrationStep> submittedCalibrationSteps = <CalibrationStep>[];
  final List<String> revokedDeviceIds = <String>[];
  var calibrationStartCount = 0;
  var snapshotReadCount = 0;

  @override
  Future<PairingSession> beginPairing(String qrPayload) async {
    pairingPayloads.add(qrPayload);
    final error = beginPairingError;
    if (error != null) {
      throw error;
    }
    final handler = onBeginPairing;
    if (handler != null) {
      return await handler(qrPayload);
    }
    return pairingSession;
  }

  @override
  Future<void> confirmPairing(String sessionId) async {
    confirmedSessionIds.add(sessionId);
    final error = confirmPairingError;
    if (error != null) {
      throw error;
    }
    await onConfirmPairing?.call(sessionId);
  }

  @override
  Future<Diagnostics> getDiagnostics() async {
    return const Diagnostics(summary: 'fake native gateway');
  }

  @override
  Future<UnlockSnapshot> getSnapshot() async {
    snapshotReadCount += 1;
    final error = snapshotError;
    if (error != null) {
      throw error;
    }
    final handler = onGetSnapshot;
    if (handler != null) {
      return await handler();
    }
    return snapshot;
  }

  @override
  Future<void> revokeDevice(String deviceId) async {
    revokedDeviceIds.add(deviceId);
    final error = revokeError;
    if (error != null) {
      throw error;
    }
    await onRevoke?.call(deviceId);
  }

  @override
  Future<void> startCalibration() async {
    calibrationStartCount += 1;
    await onStartCalibration?.call();
  }

  @override
  Future<void> submitCalibrationStep(CalibrationStep step) async {
    submittedCalibrationSteps.add(step);
    final error = calibrationSubmissionErrors[step];
    if (error != null) {
      throw error;
    }
    await onSubmitCalibration?.call(step);
  }
}
