import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/calibration/calibration_controller.dart';
import 'package:repose_unlock/features/devices/device_controller.dart';
import 'package:repose_unlock/native/native_models.dart';

import 'support/fake_native_gateway.dart';

void main() {
  group('CalibrationController', () {
    test('enforces near then far calibration order', () async {
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
      final controller = await _controllerFor(gateway);

      await controller.startCalibration();
      expect(controller.state.phase, CalibrationPhase.collectingNear);

      final acceptedOutOfOrder = await controller.submitStep(
        CalibrationStep.far,
      );
      expect(acceptedOutOfOrder, isFalse);
      expect(controller.state.phase, CalibrationPhase.collectingNear);
      expect(controller.state.message, contains('near'));
      expect(gateway.submittedCalibrationSteps, isEmpty);

      expect(await controller.submitStep(CalibrationStep.near), isTrue);
      expect(controller.state.phase, CalibrationPhase.collectingFar);
      expect(await controller.submitStep(CalibrationStep.far), isTrue);
      expect(controller.state.phase, CalibrationPhase.complete);
      expect(gateway.submittedCalibrationSteps, <CalibrationStep>[
        CalibrationStep.near,
        CalibrationStep.far,
      ]);
    });

    test('surfaces overlapping samples as an explicit retry state', () async {
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot())
        ..calibrationSubmissionErrors[CalibrationStep.far] =
            const NativeGatewayException(NativeErrorCode.calibrationOverlap);
      final controller = await _controllerFor(gateway);
      await controller.startCalibration();

      expect(await controller.submitStep(CalibrationStep.near), isTrue);
      final accepted = await controller.submitStep(CalibrationStep.far);

      expect(accepted, isFalse);
      expect(controller.state.phase, CalibrationPhase.overlapRejected);
      expect(controller.state.message, contains('overlap'));
      expect(controller.state.canRetry, isTrue);
      expect(controller.state.phase, isNot(CalibrationPhase.complete));
    });

    test(
      'requires a paired device before starting native calibration',
      () async {
        final gateway = FakeNativeGateway(snapshot: _readyUnpairedSnapshot());
        final controller = await _controllerFor(gateway);

        expect(await controller.startCalibration(), isFalse);

        expect(gateway.calibrationStartCount, 0);
        expect(controller.state.phase, CalibrationPhase.unavailable);
      },
    );

    test('capability downgrade blocks a pending calibration step', () async {
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
      final devices = await _hydratedDevices(gateway);
      final controller = CalibrationController(
        gateway: gateway,
        deviceController: devices,
      );
      await controller.startCalibration();
      gateway.snapshot = _unsupportedPairedSnapshot();
      await devices.hydrate();

      expect(await controller.submitStep(CalibrationStep.near), isFalse);

      expect(gateway.submittedCalibrationSteps, isEmpty);
      expect(controller.state.phase, CalibrationPhase.unavailable);
    });

    test('serializes double start and submit while start is pending', () async {
      final pending = Completer<void>();
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot())
        ..onStartCalibration = () => pending.future;
      final controller = await _controllerFor(gateway);

      final first = controller.startCalibration();
      expect(controller.state.isBusy, isTrue);
      expect(await controller.startCalibration(), isFalse);
      expect(await controller.submitStep(CalibrationStep.near), isFalse);
      expect(gateway.calibrationStartCount, 1);
      expect(gateway.submittedCalibrationSteps, isEmpty);
      pending.complete();

      expect(await first, isTrue);
      expect(controller.state.phase, CalibrationPhase.collectingNear);
      expect(controller.state.isBusy, isFalse);
    });

    test(
      'serializes double submit and start while submit is pending',
      () async {
        final pending = Completer<void>();
        final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
        final controller = await _controllerFor(gateway);
        await controller.startCalibration();
        gateway.onSubmitCalibration = (_) => pending.future;

        final first = controller.submitStep(CalibrationStep.near);
        expect(controller.state.isBusy, isTrue);
        expect(await controller.submitStep(CalibrationStep.near), isFalse);
        expect(await controller.startCalibration(), isFalse);
        expect(gateway.submittedCalibrationSteps, <CalibrationStep>[
          CalibrationStep.near,
        ]);
        expect(gateway.calibrationStartCount, 1);
        pending.complete();

        expect(await first, isTrue);
        expect(controller.state.phase, CalibrationPhase.collectingFar);
        expect(controller.state.isBusy, isFalse);
      },
    );

    test('downgrade during a pending step wins over late success', () async {
      final pending = Completer<void>();
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
      final devices = await _hydratedDevices(gateway);
      final controller = CalibrationController(
        gateway: gateway,
        deviceController: devices,
      );
      await controller.startCalibration();
      gateway.onSubmitCalibration = (_) => pending.future;

      final submission = controller.submitStep(CalibrationStep.near);
      gateway.snapshot = _unsupportedPairedSnapshot();
      expect(await devices.hydrate(), isTrue);
      pending.complete();

      expect(await submission, isFalse);
      expect(controller.state.phase, CalibrationPhase.unavailable);
      expect(controller.state.phase, isNot(CalibrationPhase.collectingFar));
    });

    test('newer hydration fences an older async completion', () async {
      final pending = Completer<void>();
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot())
        ..onStartCalibration = () => pending.future;
      final controller = await _controllerFor(gateway);

      final start = controller.startCalibration();
      controller.hydrateFromSnapshot(
        const CalibrationSnapshot(phase: CalibrationPhase.unavailable),
      );
      pending.complete();

      expect(await start, isFalse);
      expect(controller.state.phase, CalibrationPhase.unavailable);
      expect(controller.state.isBusy, isFalse);
    });

    test(
      'late completion after dispose is ignored without notification',
      () async {
        final pending = Completer<void>();
        final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot())
          ..onStartCalibration = () => pending.future;
        final controller = await _controllerFor(gateway);
        var notifications = 0;
        controller.addListener(() => notifications += 1);

        final start = controller.startCalibration();
        expect(notifications, 1);
        controller.dispose();
        pending.complete();

        expect(await start, isFalse);
        expect(notifications, 1);
        expect(controller.state.isBusy, isTrue);
      },
    );

    test('late submit after dispose is ignored without notification', () async {
      final pending = Completer<void>();
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
      final controller = await _controllerFor(gateway);
      await controller.startCalibration();
      gateway.onSubmitCalibration = (_) => pending.future;
      var notifications = 0;
      controller.addListener(() => notifications += 1);

      final submission = controller.submitStep(CalibrationStep.near);
      expect(notifications, 1);
      controller.dispose();
      pending.complete();

      expect(await submission, isFalse);
      expect(notifications, 1);
      expect(controller.state.phase, CalibrationPhase.collectingNear);
      expect(controller.state.isBusy, isTrue);
    });

    test('hydrates the authoritative next step after UI restart', () async {
      final gateway = FakeNativeGateway(snapshot: _readyPairedSnapshot());
      final controller = await _controllerFor(gateway);

      controller.hydrateFromSnapshot(
        const CalibrationSnapshot(phase: CalibrationPhase.collectingFar),
      );

      expect(controller.state.phase, CalibrationPhase.collectingFar);
      expect(controller.state.message, contains('far'));
    });
  });
}

const _realme = PairedDevice(
  id: 'phone-1',
  displayName: 'realme GT5 Pro',
  platform: CompanionPlatform.android,
);

UnlockSnapshot _readyPairedSnapshot() => UnlockSnapshot(
  capability: CompanionCapability.ready,
  devices: const <PairedDevice>[_realme],
  calibration: const CalibrationSnapshot(),
);

UnlockSnapshot _readyUnpairedSnapshot() => UnlockSnapshot(
  capability: CompanionCapability.ready,
  devices: const <PairedDevice>[],
  calibration: const CalibrationSnapshot(),
);

UnlockSnapshot _unsupportedPairedSnapshot() => UnlockSnapshot(
  capability: CompanionCapability.unsupportedPlatform,
  devices: const <PairedDevice>[_realme],
  calibration: const CalibrationSnapshot(),
);

Future<DeviceController> _hydratedDevices(FakeNativeGateway gateway) async {
  final controller = DeviceController(gateway: gateway);
  expect(await controller.hydrate(), isTrue);
  return controller;
}

Future<CalibrationController> _controllerFor(FakeNativeGateway gateway) async {
  return CalibrationController(
    gateway: gateway,
    deviceController: await _hydratedDevices(gateway),
  );
}
