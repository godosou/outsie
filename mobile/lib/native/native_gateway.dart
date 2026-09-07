import 'native_models.dart';

abstract interface class NativeGateway {
  Future<UnlockSnapshot> getSnapshot();
  Future<PairingSession> beginPairing(String qrPayload);
  Future<void> confirmPairing(String sessionId);
  Future<void> startCalibration();
  Future<void> submitCalibrationStep(CalibrationStep step);
  Future<void> revokeDevice(String deviceId);
  Future<Diagnostics> getDiagnostics();
}

/// Safe placeholder until the Kotlin and Swift adapters land.
///
/// It exposes only domain operations and always fails closed for mutations.
final class UnavailableNativeGateway implements NativeGateway {
  const UnavailableNativeGateway();

  static const _unavailable = NativeGatewayException(
    NativeErrorCode.bridgeUnavailable,
    safeMessage: 'The native phone-key service is not connected yet.',
  );

  @override
  Future<PairingSession> beginPairing(String qrPayload) =>
      Future<PairingSession>.error(_unavailable);

  @override
  Future<void> confirmPairing(String sessionId) =>
      Future<void>.error(_unavailable);

  @override
  Future<Diagnostics> getDiagnostics() async => const Diagnostics(
    summary: 'Native phone-key service unavailable in the Flutter shell.',
  );

  @override
  Future<UnlockSnapshot> getSnapshot() async => UnlockSnapshot(
    capability: CompanionCapability.nativeBridgeUnavailable,
    devices: const <PairedDevice>[],
    calibration: const CalibrationSnapshot(),
  );

  @override
  Future<void> revokeDevice(String deviceId) =>
      Future<void>.error(_unavailable);

  @override
  Future<void> startCalibration() => Future<void>.error(_unavailable);

  @override
  Future<void> submitCalibrationStep(CalibrationStep step) =>
      Future<void>.error(_unavailable);
}
