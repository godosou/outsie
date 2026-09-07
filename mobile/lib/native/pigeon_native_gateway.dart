import 'dart:io';

import 'package:flutter/services.dart';
import 'package:repose_unlock_native/repose_unlock_native.dart' as pigeon;

import 'native_gateway.dart';
import 'native_models.dart';

abstract interface class ReposeUnlockPigeonClient {
  Future<pigeon.NativeUnlockSnapshot> getSnapshot();
  Future<pigeon.NativePairingSession> beginPairing(String qrPayload);
  Future<void> confirmPairing(String sessionId);
  Future<void> startCalibration();
  Future<void> submitCalibrationStep(pigeon.NativeCalibrationStep step);
  Future<void> revokeDevice(String deviceId);
  Future<pigeon.NativeDiagnostics> getDiagnostics();
}

final class GeneratedReposeUnlockPigeonClient
    implements ReposeUnlockPigeonClient {
  GeneratedReposeUnlockPigeonClient({pigeon.ReposeUnlockHostApi? api})
    : _api = api ?? pigeon.ReposeUnlockHostApi();

  final pigeon.ReposeUnlockHostApi _api;

  @override
  Future<pigeon.NativePairingSession> beginPairing(String qrPayload) =>
      _api.beginPairing(qrPayload);

  @override
  Future<void> confirmPairing(String sessionId) =>
      _api.confirmPairing(sessionId);

  @override
  Future<pigeon.NativeDiagnostics> getDiagnostics() => _api.getDiagnostics();

  @override
  Future<pigeon.NativeUnlockSnapshot> getSnapshot() => _api.getSnapshot();

  @override
  Future<void> revokeDevice(String deviceId) => _api.revokeDevice(deviceId);

  @override
  Future<void> startCalibration() => _api.startCalibration();

  @override
  Future<void> submitCalibrationStep(pigeon.NativeCalibrationStep step) =>
      _api.submitCalibrationStep(step);
}

final class PigeonNativeGateway implements NativeGateway {
  PigeonNativeGateway({ReposeUnlockPigeonClient? client})
    : _client = client ?? GeneratedReposeUnlockPigeonClient();

  final ReposeUnlockPigeonClient _client;

  @override
  Future<UnlockSnapshot> getSnapshot() => _guard(() async {
    final snapshot = await _client.getSnapshot();
    return UnlockSnapshot(
      capability: _capability(snapshot.capability),
      devices: snapshot.devices.map(_device),
      calibration: CalibrationSnapshot(
        phase: _calibrationPhase(snapshot.calibration.phase),
      ),
      pendingPairing: snapshot.pendingPairing == null
          ? null
          : _pairingSession(snapshot.pendingPairing!),
    );
  });

  @override
  Future<PairingSession> beginPairing(String qrPayload) => _guard(() async {
    return _pairingSession(await _client.beginPairing(qrPayload));
  });

  @override
  Future<void> confirmPairing(String sessionId) =>
      _guard(() => _client.confirmPairing(sessionId));

  @override
  Future<void> startCalibration() => _guard(_client.startCalibration);

  @override
  Future<void> submitCalibrationStep(CalibrationStep step) => _guard(
    () => _client.submitCalibrationStep(switch (step) {
      CalibrationStep.near => pigeon.NativeCalibrationStep.near,
      CalibrationStep.far => pigeon.NativeCalibrationStep.far,
    }),
  );

  @override
  Future<void> revokeDevice(String deviceId) =>
      _guard(() => _client.revokeDevice(deviceId));

  @override
  Future<Diagnostics> getDiagnostics() => _guard(() async {
    final diagnostics = await _client.getDiagnostics();
    return Diagnostics(summary: diagnostics.summary);
  });

  Future<T> _guard<T>(Future<T> Function() operation) async {
    try {
      return await operation();
    } on PlatformException catch (error) {
      final code = _errorCode(error.code);
      throw NativeGatewayException(code, safeMessage: _safeMessage(code));
    } catch (_) {
      throw const NativeGatewayException(
        NativeErrorCode.unknown,
        safeMessage: 'The native phone-key operation failed safely.',
      );
    }
  }
}

NativeGateway createNativeGateway({
  bool? isAndroid,
  ReposeUnlockPigeonClient? pigeonClient,
}) {
  if (!(isAndroid ?? Platform.isAndroid)) {
    return const UnavailableNativeGateway();
  }
  return PigeonNativeGateway(client: pigeonClient);
}

PairingSession _pairingSession(pigeon.NativePairingSession session) =>
    PairingSession(
      sessionId: session.sessionId,
      deviceName: session.deviceName,
      expiresAt: DateTime.fromMillisecondsSinceEpoch(
        session.expiresAtEpochMillis,
        isUtc: true,
      ),
    );

PairedDevice _device(pigeon.NativePairedDevice device) => PairedDevice(
  id: device.id,
  displayName: device.displayName,
  platform: switch (device.platform) {
    pigeon.NativeCompanionPlatform.android => CompanionPlatform.android,
    pigeon.NativeCompanionPlatform.ios => CompanionPlatform.ios,
  },
);

CompanionCapability _capability(pigeon.NativeCompanionCapability capability) =>
    switch (capability) {
      pigeon.NativeCompanionCapability.ready => CompanionCapability.ready,
      pigeon.NativeCompanionCapability.bluetoothUnavailable =>
        CompanionCapability.bluetoothUnavailable,
      pigeon.NativeCompanionCapability.secureHardwareUnavailable =>
        CompanionCapability.secureHardwareUnavailable,
      pigeon.NativeCompanionCapability.backgroundExecutionUnavailable =>
        CompanionCapability.backgroundExecutionUnavailable,
      pigeon.NativeCompanionCapability.unsupportedPlatform =>
        CompanionCapability.unsupportedPlatform,
      pigeon.NativeCompanionCapability.nativeBridgeUnavailable =>
        CompanionCapability.nativeBridgeUnavailable,
    };

CalibrationPhase _calibrationPhase(pigeon.NativeCalibrationPhase phase) =>
    switch (phase) {
      pigeon.NativeCalibrationPhase.idle => CalibrationPhase.idle,
      pigeon.NativeCalibrationPhase.collectingNear =>
        CalibrationPhase.collectingNear,
      pigeon.NativeCalibrationPhase.collectingFar =>
        CalibrationPhase.collectingFar,
      pigeon.NativeCalibrationPhase.complete => CalibrationPhase.complete,
      pigeon.NativeCalibrationPhase.overlapRejected =>
        CalibrationPhase.overlapRejected,
      pigeon.NativeCalibrationPhase.unavailable => CalibrationPhase.unavailable,
      pigeon.NativeCalibrationPhase.failed => CalibrationPhase.failed,
    };

NativeErrorCode _errorCode(String code) => switch (code) {
  'qrExpired' => NativeErrorCode.qrExpired,
  'qrAlreadyUsed' => NativeErrorCode.qrAlreadyUsed,
  'capabilityUnavailable' => NativeErrorCode.capabilityUnavailable,
  'unsupported' => NativeErrorCode.unsupported,
  'calibrationOutOfOrder' => NativeErrorCode.calibrationOutOfOrder,
  'calibrationOverlap' => NativeErrorCode.calibrationOverlap,
  'deviceNotFound' => NativeErrorCode.deviceNotFound,
  'bridgeUnavailable' => NativeErrorCode.bridgeUnavailable,
  _ => NativeErrorCode.unknown,
};

String _safeMessage(NativeErrorCode code) => switch (code) {
  NativeErrorCode.qrExpired => 'This pairing code has expired.',
  NativeErrorCode.qrAlreadyUsed => 'This pairing code has already been used.',
  NativeErrorCode.capabilityUnavailable =>
    'This phone-key capability is unavailable.',
  NativeErrorCode.unsupported =>
    'This phone-key operation is not supported on this device.',
  NativeErrorCode.calibrationOutOfOrder =>
    'Calibration steps must be completed in order.',
  NativeErrorCode.calibrationOverlap =>
    'The near and far calibration samples overlap. Try again.',
  NativeErrorCode.deviceNotFound => 'The paired device was not found.',
  NativeErrorCode.bridgeUnavailable =>
    'The native phone-key service is unavailable.',
  NativeErrorCode.unknown => 'The native phone-key operation failed safely.',
};
