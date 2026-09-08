import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/devices/device_controller.dart';
import 'package:repose_unlock/features/pairing/pairing_controller.dart';
import 'package:repose_unlock/native/native_gateway.dart';
import 'package:repose_unlock/native/native_models.dart';
import 'package:repose_unlock/native/pigeon_native_gateway.dart';
import 'package:repose_unlock_native/repose_unlock_native.dart' as pigeon;

void main() {
  test('generated snapshot is converted to the Task 9 domain model', () async {
    final client = _FakePigeonClient(
      snapshot: pigeon.NativeUnlockSnapshot(
        capability:
            pigeon.NativeCompanionCapability.backgroundExecutionUnavailable,
        devices: <pigeon.NativePairedDevice>[],
        calibration: pigeon.NativeCalibrationSnapshot(
          phase: pigeon.NativeCalibrationPhase.unavailable,
        ),
      ),
    );

    final snapshot = await PigeonNativeGateway(client: client).getSnapshot();

    expect(
      snapshot.capability,
      CompanionCapability.backgroundExecutionUnavailable,
    );
    expect(snapshot.devices, isEmpty);
    expect(snapshot.calibration.phase, CalibrationPhase.unavailable);
  });

  test('native exception payload is replaced by Dart-owned copy', () async {
    const expected = <String, (NativeErrorCode, String)>{
      'qrExpired': (
        NativeErrorCode.qrExpired,
        'This pairing QR code has expired.',
      ),
      'qrAlreadyUsed': (
        NativeErrorCode.qrAlreadyUsed,
        'This pairing QR code has already been used.',
      ),
      'capabilityUnavailable': (
        NativeErrorCode.capabilityUnavailable,
        'This phone-key capability is unavailable.',
      ),
      'unsupported': (
        NativeErrorCode.unsupported,
        'This phone-key operation is not supported on this device.',
      ),
      'calibrationOutOfOrder': (
        NativeErrorCode.calibrationOutOfOrder,
        'Calibration steps must be completed in order.',
      ),
      'calibrationOverlap': (
        NativeErrorCode.calibrationOverlap,
        'The near and far calibration samples overlap. Try again.',
      ),
      'deviceNotFound': (
        NativeErrorCode.deviceNotFound,
        'The paired device was not found.',
      ),
      'bridgeUnavailable': (
        NativeErrorCode.bridgeUnavailable,
        'The native phone-key service is unavailable.',
      ),
      'associationBusy': (
        NativeErrorCode.associationBusy,
        'A companion association request is already open.',
      ),
      'bluetoothPermissionDenied': (
        NativeErrorCode.bluetoothPermissionDenied,
        'Nearby devices access is required to find your Mac.',
      ),
      'associationCancelled': (
        NativeErrorCode.associationCancelled,
        'No Mac was associated. You can try again.',
      ),
      'activityUnavailable': (
        NativeErrorCode.activityUnavailable,
        'Open Repose on your phone before starting system association.',
      ),
      'associationDiscoveryFailed': (
        NativeErrorCode.associationDiscoveryFailed,
        'Repose could not find a Mac advertising the phone-key service.',
      ),
      'associationConfigurationFailed': (
        NativeErrorCode.associationConfigurationFailed,
        'Android did not confirm the companion association.',
      ),
    };

    for (final entry in expected.entries) {
      final secret = 'native-secret-${entry.key}';
      final error = await _captureGatewayError(
        PlatformException(
          code: entry.key,
          message: secret,
          details: <String, Object>{'private': '$secret-details'},
        ),
      );

      expect(error.code, entry.value.$1);
      expect(error.safeMessage, entry.value.$2);
      expect(error.safeMessage, isNot(contains(secret)));
      expect(error.toString(), isNot(contains(secret)));
    }
  });

  test('unknown native errors use one fixed generic message', () async {
    const secret = 'raw keystore exception and QR contents';
    final error = await _captureGatewayError(
      PlatformException(
        code: 'unexpectedNativeFailure',
        message: secret,
        details: secret,
      ),
    );

    expect(error.code, NativeErrorCode.unknown);
    expect(error.safeMessage, 'The native phone-key operation failed safely.');
    expect(error.safeMessage, isNot(contains(secret)));
  });

  test('pairing controller never receives a raw native message', () async {
    const secret = 'QR=private-token; keystore=internal-provider-error';
    final gateway = PigeonNativeGateway(
      client: _FakePigeonClient(
        snapshot: pigeon.NativeUnlockSnapshot(
          capability: pigeon.NativeCompanionCapability.ready,
          devices: <pigeon.NativePairedDevice>[],
          calibration: pigeon.NativeCalibrationSnapshot(
            phase: pigeon.NativeCalibrationPhase.idle,
          ),
        ),
        error: PlatformException(
          code: 'capabilityUnavailable',
          message: secret,
          details: secret,
        ),
      ),
    );
    final devices = DeviceController(gateway: gateway);
    final pairing = PairingController(
      gateway: gateway,
      deviceController: devices,
    );
    addTearDown(devices.dispose);
    addTearDown(pairing.dispose);
    expect(await devices.hydrate(), isTrue);

    expect(await pairing.beginPairing('repose://pair/redacted'), isFalse);
    expect(pairing.state.message, 'This phone-key capability is unavailable.');
    expect(pairing.state.message, isNot(contains(secret)));
  });

  test('gateway factory selects Pigeon only for Android', () {
    final client = _FakePigeonClient();

    expect(
      createNativeGateway(isAndroid: true, pigeonClient: client),
      isA<PigeonNativeGateway>(),
    );
    expect(
      createNativeGateway(isAndroid: false, pigeonClient: client),
      isA<UnavailableNativeGateway>(),
    );
  });

  test(
    'system companion association is exposed separately from secure pairing',
    () async {
      final client = _FakePigeonClient(associationSucceeds: true);

      await PigeonNativeGateway(client: client).requestCompanionAssociation();

      expect(client.associationRequestCount, 1);
    },
  );

  test('maps pending Mac acceptance to a stable retryable error', () async {
    final error = await _captureGatewayError(
      PlatformException(code: 'pairingNotAccepted'),
    );

    expect(error.code, NativeErrorCode.pairingNotAccepted);
    expect(error.safeMessage, contains('connecting'));
  });
}

Future<NativeGatewayException> _captureGatewayError(
  PlatformException platformError,
) async {
  final gateway = PigeonNativeGateway(
    client: _FakePigeonClient(error: platformError),
  );
  try {
    await gateway.beginPairing('repose://pair/redacted');
    fail('the fail-closed fake unexpectedly returned');
  } on NativeGatewayException catch (error) {
    return error;
  }
}

final class _FakePigeonClient implements ReposeUnlockPigeonClient {
  _FakePigeonClient({
    this.snapshot,
    this.error,
    this.associationSucceeds = false,
  });

  final pigeon.NativeUnlockSnapshot? snapshot;
  final PlatformException? error;
  final bool associationSucceeds;
  var associationRequestCount = 0;

  Never _fail() => throw error ?? StateError('unexpected fake call');

  @override
  Future<pigeon.NativePairingSession> beginPairing(String qrPayload) async =>
      _fail();

  @override
  Future<void> confirmPairing(String sessionId) async => _fail();

  @override
  Future<void> requestCompanionAssociation() async {
    associationRequestCount += 1;
    if (!associationSucceeds) _fail();
  }

  @override
  Future<pigeon.NativeDiagnostics> getDiagnostics() async =>
      pigeon.NativeDiagnostics(summary: 'test');

  @override
  Future<pigeon.NativeUnlockSnapshot> getSnapshot() async =>
      snapshot ?? _fail();

  @override
  Future<void> revokeDevice(String deviceId) async => _fail();

  @override
  Future<void> startCalibration() async => _fail();

  @override
  Future<void> submitCalibrationStep(pigeon.NativeCalibrationStep step) async =>
      _fail();
}
