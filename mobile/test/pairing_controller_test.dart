import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/devices/device_controller.dart';
import 'package:repose_unlock/features/pairing/pairing_controller.dart';
import 'package:repose_unlock/native/native_models.dart';

import 'support/fake_native_gateway.dart';

void main() {
  group('PairingController', () {
    test('marks an expired QR session and never confirms it', () async {
      final now = DateTime.utc(2026, 9, 8, 10);
      final gateway = FakeNativeGateway(
        snapshot: _readySnapshot(),
        pairingSession: PairingSession(
          sessionId: 'expired-session',
          deviceName: 'realme GT5 Pro',
          expiresAt: now,
        ),
      );
      final devices = await _hydratedDevices(gateway);
      final controller = PairingController(
        gateway: gateway,
        deviceController: devices,
        clock: () => now,
      );

      await controller.beginPairing('repose://pair/expired');
      final confirmed = await controller.confirmPairing();

      expect(controller.state.phase, PairingPhase.expired);
      expect(confirmed, isFalse);
      expect(gateway.confirmedSessionIds, isEmpty);
    });

    test(
      'confirms explicitly then refreshes the authoritative device snapshot',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        final gateway = FakeNativeGateway(
          snapshot: _readySnapshot(),
          pairingSession: PairingSession(
            sessionId: 'first-pair',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 2)),
          ),
        );
        final devices = await _hydratedDevices(gateway);
        gateway.onConfirmPairing = (_) {
          gateway.snapshot = _readySnapshot(
            devices: const <PairedDevice>[_realme],
          );
        };
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );

        await controller.beginPairing('repose://pair/one-time-token');

        expect(controller.state.phase, PairingPhase.awaitingConfirmation);
        expect(controller.state.deviceName, 'realme GT5 Pro');
        expect(gateway.confirmedSessionIds, isEmpty);
        expect(devices.state.devices, isEmpty);

        final confirmed = await controller.confirmPairing();

        expect(confirmed, isTrue);
        expect(controller.state.phase, PairingPhase.paired);
        expect(gateway.confirmedSessionIds, <String>['first-pair']);
        expect(gateway.snapshotReadCount, 2);
        expect(devices.state.devices, const <PairedDevice>[_realme]);
        expect(devices.state.gate.canCalibrate, isTrue);
      },
    );

    test(
      'refresh failure after native confirmation remains fail-closed',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        final gateway = FakeNativeGateway(
          snapshot: _readySnapshot(),
          pairingSession: PairingSession(
            sessionId: 'refresh-fails',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 2)),
          ),
        );
        final devices = await _hydratedDevices(gateway);
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );
        await controller.beginPairing('repose://pair/refresh-fails');
        gateway.snapshotError = const NativeGatewayException(
          NativeErrorCode.bridgeUnavailable,
        );

        final confirmed = await controller.confirmPairing();

        expect(confirmed, isFalse);
        expect(gateway.confirmedSessionIds, <String>['refresh-fails']);
        expect(controller.state.phase, PairingPhase.failed);
        expect(controller.state.message, contains('refresh'));
        expect(devices.state.gate.canPair, isFalse);
        expect(devices.state.gate.canCalibrate, isFalse);
      },
    );

    test(
      'an unchanged unrelated device snapshot cannot confirm this pairing',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        const existingDevice = PairedDevice(
          id: 'phone-existing',
          displayName: 'Existing iPhone',
          platform: CompanionPlatform.ios,
        );
        final gateway = FakeNativeGateway(
          snapshot: _readySnapshot(devices: const [existingDevice]),
          pairingSession: PairingSession(
            sessionId: 'unchanged-snapshot',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 2)),
          ),
        );
        final devices = await _hydratedDevices(gateway);
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );
        await controller.beginPairing('repose://pair/unchanged');

        final confirmed = await controller.confirmPairing();

        expect(confirmed, isFalse);
        expect(controller.state.phase, PairingPhase.failed);
        expect(devices.state.devices, const [existingDevice]);
      },
    );

    test('rechecks expiry at the confirmation boundary', () async {
      var now = DateTime.utc(2026, 9, 8, 10);
      final expiresAt = now.add(const Duration(minutes: 2));
      final gateway = FakeNativeGateway(
        snapshot: _readySnapshot(),
        pairingSession: PairingSession(
          sessionId: 'boundary-session',
          deviceName: 'realme GT5 Pro',
          expiresAt: expiresAt,
        ),
      );
      final devices = await _hydratedDevices(gateway);
      final controller = PairingController(
        gateway: gateway,
        deviceController: devices,
        clock: () => now,
      );
      await controller.beginPairing('repose://pair/boundary');

      now = expiresAt;
      final confirmed = await controller.confirmPairing();

      expect(confirmed, isFalse);
      expect(controller.state.phase, PairingPhase.expired);
      expect(gateway.confirmedSessionIds, isEmpty);
    });

    test('maps a native expiry race to the same expired state', () async {
      final now = DateTime.utc(2026, 9, 8, 10);
      final gateway =
          FakeNativeGateway(
              snapshot: _readySnapshot(),
              pairingSession: PairingSession(
                sessionId: 'native-race',
                deviceName: 'realme GT5 Pro',
                expiresAt: now.add(const Duration(minutes: 2)),
              ),
            )
            ..confirmPairingError = const NativeGatewayException(
              NativeErrorCode.qrExpired,
            );
      final devices = await _hydratedDevices(gateway);
      final controller = PairingController(
        gateway: gateway,
        deviceController: devices,
        clock: () => now,
      );
      await controller.beginPairing('repose://pair/native-race');

      final confirmed = await controller.confirmPairing();

      expect(confirmed, isFalse);
      expect(controller.state.phase, PairingPhase.expired);
      expect(controller.state.message, isNotEmpty);
    });

    test('capability downgrade blocks pending confirmation natively', () async {
      final now = DateTime.utc(2026, 9, 8, 10);
      final gateway = FakeNativeGateway(
        snapshot: _unsupportedSnapshot(
          pendingPairing: PairingSession(
            sessionId: 'blocked-confirm',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 1)),
          ),
        ),
      );
      final devices = await _hydratedDevices(gateway);
      final controller = PairingController(
        gateway: gateway,
        deviceController: devices,
        clock: () => now,
      )..hydrateFromSnapshot(devices.state.snapshot?.pendingPairing);

      expect(await controller.confirmPairing(), isFalse);

      expect(gateway.confirmedSessionIds, isEmpty);
      expect(controller.state.phase, PairingPhase.failed);
      expect(controller.state.message, contains('unavailable'));
    });

    test('hydrates a pending native confirmation after UI restart', () async {
      final now = DateTime.utc(2026, 9, 8, 10);
      final gateway = FakeNativeGateway(snapshot: _readySnapshot());
      final devices = await _hydratedDevices(gateway);
      final controller = PairingController(
        gateway: gateway,
        deviceController: devices,
        clock: () => now,
      );

      controller.hydrateFromSnapshot(
        PairingSession(
          sessionId: 'restored-session',
          deviceName: 'realme GT5 Pro',
          expiresAt: now.add(const Duration(minutes: 1)),
        ),
      );

      expect(controller.state.phase, PairingPhase.awaitingConfirmation);
      expect(controller.state.deviceName, 'realme GT5 Pro');
      expect(gateway.pairingPayloads, isEmpty);
      expect(gateway.confirmedSessionIds, isEmpty);
    });

    test(
      'late begin completion cannot overwrite newer hydrated state',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        final pending = Completer<PairingSession>();
        final gateway = FakeNativeGateway(snapshot: _readySnapshot())
          ..onBeginPairing = (_) => pending.future;
        final devices = await _hydratedDevices(gateway);
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );

        final begin = controller.beginPairing('repose://pair/slow');
        expect(controller.state.phase, PairingPhase.starting);
        gateway.snapshot = _unsupportedSnapshot();
        expect(await devices.hydrate(), isTrue);
        controller.hydrateFromSnapshot(null);
        pending.complete(
          PairingSession(
            sessionId: 'stale-session',
            deviceName: 'stale phone',
            expiresAt: now.add(const Duration(minutes: 1)),
          ),
        );

        expect(await begin, isFalse);
        expect(controller.state.phase, PairingPhase.idle);
        expect(controller.state.session, isNull);
        expect(devices.state.gate.canPair, isFalse);
      },
    );

    test(
      'late confirmation after dispose skips refresh and notification',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        final pending = Completer<void>();
        final gateway = FakeNativeGateway(
          snapshot: _readySnapshot(),
          pairingSession: PairingSession(
            sessionId: 'dispose-confirm',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 1)),
          ),
        )..onConfirmPairing = (_) => pending.future;
        final devices = await _hydratedDevices(gateway);
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );
        await controller.beginPairing('repose://pair/dispose-confirm');
        var notifications = 0;
        controller.addListener(() => notifications += 1);

        final confirmation = controller.confirmPairing();
        expect(notifications, 1);
        controller.dispose();
        pending.complete();

        expect(await confirmation, isFalse);
        expect(notifications, 1);
        expect(controller.state.phase, PairingPhase.confirming);
        expect(gateway.snapshotReadCount, 1);
      },
    );

    test(
      'late completion after dispose is ignored without notification',
      () async {
        final now = DateTime.utc(2026, 9, 8, 10);
        final pending = Completer<PairingSession>();
        final gateway = FakeNativeGateway(snapshot: _readySnapshot())
          ..onBeginPairing = (_) => pending.future;
        final devices = await _hydratedDevices(gateway);
        final controller = PairingController(
          gateway: gateway,
          deviceController: devices,
          clock: () => now,
        );
        var notifications = 0;
        controller.addListener(() => notifications += 1);

        final begin = controller.beginPairing('repose://pair/dispose');
        expect(notifications, 1);
        controller.dispose();
        pending.complete(
          PairingSession(
            sessionId: 'disposed-session',
            deviceName: 'realme GT5 Pro',
            expiresAt: now.add(const Duration(minutes: 1)),
          ),
        );

        expect(await begin, isFalse);
        expect(notifications, 1);
        expect(controller.state.phase, PairingPhase.starting);
      },
    );
  });
}

const _realme = PairedDevice(
  id: 'phone-1',
  displayName: 'realme GT5 Pro',
  platform: CompanionPlatform.android,
);

UnlockSnapshot _readySnapshot({
  Iterable<PairedDevice> devices = const <PairedDevice>[],
  PairingSession? pendingPairing,
}) => UnlockSnapshot(
  capability: CompanionCapability.ready,
  devices: devices,
  calibration: const CalibrationSnapshot(),
  pendingPairing: pendingPairing,
);

UnlockSnapshot _unsupportedSnapshot({PairingSession? pendingPairing}) =>
    UnlockSnapshot(
      capability: CompanionCapability.unsupportedPlatform,
      devices: const <PairedDevice>[],
      calibration: const CalibrationSnapshot(),
      pendingPairing: pendingPairing,
    );

Future<DeviceController> _hydratedDevices(FakeNativeGateway gateway) async {
  final controller = DeviceController(gateway: gateway);
  expect(await controller.hydrate(), isTrue);
  return controller;
}
