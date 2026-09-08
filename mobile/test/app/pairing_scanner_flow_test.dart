import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/repose_unlock_app.dart';
import 'package:repose_unlock/features/pairing/pairing_scanner.dart';
import 'package:repose_unlock/native/native_models.dart';

import '../support/fake_native_gateway.dart';

void main() {
  testWidgets('lost native pairing restores scanning without a confirmation tap', (
    tester,
  ) async {
    final gateway = _readyGateway();
    gateway.snapshot = _pendingSnapshot(gateway.pairingSession);
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('confirmPairingButton')), findsOneWidget);

    gateway.snapshot = UnlockSnapshot(
      capability: CompanionCapability.ready,
      devices: const <PairedDevice>[],
      calibration: const CalibrationSnapshot(),
    );
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();

    expect(find.byKey(const Key('confirmPairingButton')), findsNothing);
    expect(find.textContaining('Pairing connection ended'), findsOneWidget);
    expect(
      tester.widget<FilledButton>(
        find.byKey(const Key('scanPairingQrButton')),
      ).onPressed,
      isNotNull,
    );
    expect(gateway.confirmedSessionIds, isEmpty);
    final readCount = gateway.snapshotReadCount;
    await tester.pump(const Duration(seconds: 3));
    expect(gateway.snapshotReadCount, readCount);
  });

  testWidgets('a pending status read does not overlap or overwrite confirmation', (
    tester,
  ) async {
    final gateway = _readyGateway();
    gateway.snapshot = _pendingSnapshot(gateway.pairingSession);
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    final staleRead = Completer<UnlockSnapshot>();
    gateway.onGetSnapshot = () => staleRead.future;
    await tester.pump(const Duration(seconds: 1));
    await tester.pump(const Duration(seconds: 3));
    expect(gateway.snapshotReadCount, 2);

    final confirm = Completer<void>();
    gateway.onConfirmPairing = (_) => confirm.future;
    final button = find.byKey(const Key('confirmPairingButton'));
    await tester.ensureVisible(button);
    await tester.tap(button);
    await tester.pump();
    staleRead.complete(UnlockSnapshot.empty());
    await tester.pump();
    expect(find.textContaining('Pairing connection ended'), findsNothing);

    gateway.onGetSnapshot = null;
    gateway.snapshot = UnlockSnapshot(
      capability: CompanionCapability.ready,
      devices: const <PairedDevice>[
        PairedDevice(
          id: 'new-mac',
          displayName: 'realme GT5 Pro',
          platform: CompanionPlatform.android,
        ),
      ],
      calibration: const CalibrationSnapshot(),
    );
    confirm.complete();
    await tester.pumpAndSettle();
    expect(find.text('Pairing confirmed'), findsOneWidget);
  });

  testWidgets('unmount cancels pairing polling and ignores a late status result', (
    tester,
  ) async {
    final gateway = _readyGateway();
    gateway.snapshot = _pendingSnapshot(gateway.pairingSession);
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    final pending = Completer<UnlockSnapshot>();
    gateway.onGetSnapshot = () => pending.future;
    await tester.pump(const Duration(seconds: 1));
    expect(gateway.snapshotReadCount, 2);

    await tester.pumpWidget(const SizedBox.shrink());
    pending.complete(UnlockSnapshot.empty());
    await tester.pump(const Duration(seconds: 3));
    expect(gateway.snapshotReadCount, 2);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'camera explanation precedes permission and scanner submits one valid v1 QR',
    (tester) async {
      final gateway = _readyGateway();
      final camera = FakePairingCameraAccess(CameraAccessOutcome.granted);

      await tester.pumpWidget(
        ReposeUnlockApp(
          gateway: gateway,
          pairingCameraAccess: camera,
          pairingScannerBuilder: _scriptedScanner,
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(const Key('pairingQrField')), findsNothing);
      expect(find.byKey(const Key('beginPairingButton')), findsNothing);
      final scanButton = find.byKey(const Key('scanPairingQrButton'));
      await tester.ensureVisible(scanButton);
      await tester.tap(scanButton);
      await _pumpTransition(tester);

      expect(
        find.byKey(const Key('cameraPermissionRationaleDialog')),
        findsOneWidget,
      );
      expect(camera.requestCount, 0);

      await tester.tap(find.byKey(const Key('continueToCameraButton')));
      await _pumpTransition(tester);

      expect(camera.requestCount, 1);
      expect(find.byKey(const Key('pairingScannerPage')), findsOneWidget);

      await tester.tap(find.byKey(const Key('emitInvalidPairingQr')));
      await tester.pump();
      expect(find.byKey(const Key('invalidPairingQrMessage')), findsOneWidget);
      expect(gateway.pairingPayloads, isEmpty);

      await tester.tap(find.byKey(const Key('emitDuplicateValidPairingQr')));
      await tester.pumpAndSettle();

      expect(gateway.pairingPayloads, <String>[
        'repose://pair/v1/opaque-token',
      ]);
      expect(find.byKey(const Key('pairingScannerPage')), findsNothing);
      expect(find.textContaining('Confirm realme GT5 Pro'), findsOneWidget);
    },
  );

  testWidgets('a denied camera request stays in app and explains retry', (
    tester,
  ) async {
    final gateway = _readyGateway();
    final camera = FakePairingCameraAccess(CameraAccessOutcome.denied);

    await tester.pumpWidget(
      ReposeUnlockApp(
        gateway: gateway,
        pairingCameraAccess: camera,
        pairingScannerBuilder: _scriptedScanner,
      ),
    );
    await tester.pumpAndSettle();

    await _openScanner(tester);

    expect(
      find.byKey(const Key('cameraPermissionDeniedDialog')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('pairingScannerPage')), findsNothing);
    expect(gateway.pairingPayloads, isEmpty);
    expect(camera.openSettingsCount, 0);
  });

  testWidgets('a permanently denied camera request links to app settings', (
    tester,
  ) async {
    final gateway = _readyGateway();
    final camera = FakePairingCameraAccess(
      CameraAccessOutcome.permanentlyDenied,
    );

    await tester.pumpWidget(
      ReposeUnlockApp(
        gateway: gateway,
        pairingCameraAccess: camera,
        pairingScannerBuilder: _scriptedScanner,
      ),
    );
    await tester.pumpAndSettle();

    await _openScanner(tester);

    expect(
      find.byKey(const Key('cameraPermissionPermanentlyDeniedDialog')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const Key('openCameraSettingsButton')));
    await tester.pumpAndSettle();

    expect(camera.openSettingsCount, 1);
    expect(gateway.pairingPayloads, isEmpty);
  });
}

Future<void> _openScanner(WidgetTester tester) async {
  final scanButton = find.byKey(const Key('scanPairingQrButton'));
  await tester.ensureVisible(scanButton);
  await tester.tap(scanButton);
  await _pumpTransition(tester);
  await tester.tap(find.byKey(const Key('continueToCameraButton')));
  await _pumpTransition(tester);
}

Future<void> _pumpTransition(WidgetTester tester) async {
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
}

Widget _scriptedScanner(
  BuildContext context,
  ValueChanged<String?> onDetected,
) {
  return Column(
    mainAxisSize: MainAxisSize.min,
    children: <Widget>[
      FilledButton(
        key: const Key('emitInvalidPairingQr'),
        onPressed: () => onDetected('https://example.com/not-repose'),
        child: const Text('invalid'),
      ),
      FilledButton(
        key: const Key('emitDuplicateValidPairingQr'),
        onPressed: () {
          onDetected('repose://pair/v1/opaque-token');
          onDetected('repose://pair/v1/opaque-token');
        },
        child: const Text('valid twice'),
      ),
    ],
  );
}

FakeNativeGateway _readyGateway() => FakeNativeGateway(
  snapshot: UnlockSnapshot(
    capability: CompanionCapability.ready,
    devices: const <PairedDevice>[],
    calibration: const CalibrationSnapshot(),
  ),
  pairingSession: PairingSession(
    sessionId: 'scan-session',
    deviceName: 'realme GT5 Pro',
    expiresAt: DateTime.utc(2099),
  ),
);

UnlockSnapshot _pendingSnapshot(PairingSession session) => UnlockSnapshot(
  capability: CompanionCapability.ready,
  devices: const <PairedDevice>[],
  calibration: const CalibrationSnapshot(),
  pendingPairing: session,
);

class FakePairingCameraAccess implements PairingCameraAccess {
  FakePairingCameraAccess(this.outcome);

  final CameraAccessOutcome outcome;
  var requestCount = 0;
  var openSettingsCount = 0;

  @override
  Future<CameraAccessOutcome> request() async {
    requestCount += 1;
    return outcome;
  }

  @override
  Future<bool> openSettings() async {
    openSettingsCount += 1;
    return true;
  }
}
