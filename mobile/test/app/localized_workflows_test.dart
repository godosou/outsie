import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/app/app_text.dart';
import 'package:repose_unlock/app/repose_unlock_app.dart';
import 'package:repose_unlock/native/native_models.dart';
import '../support/fake_native_gateway.dart';

void main() {
  testWidgets('device names remain verbatim in Chinese UI', (tester) async {
    tester.platformDispatcher.localesTestValue = const [Locale('zh', 'CN')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(
      ReposeUnlockApp(
        gateway: FakeNativeGateway(
          snapshot: UnlockSnapshot(
            capability: CompanionCapability.ready,
            devices: const [
              PairedDevice(
                id: '1',
                displayName: 'Revoke Office Mac',
                platform: CompanionPlatform.android,
              ),
            ],
            calibration: const CalibrationSnapshot(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Revoke Office Mac'), findsNWidgets(2));
    expect(find.text('移除 Office Mac'), findsNothing);
  });

  testWidgets('Chinese errors never expose developer-only operation copy', (
    tester,
  ) async {
    tester.platformDispatcher.localesTestValue = const [Locale('zh')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(ReposeUnlockApp(gateway: FakeNativeGateway()));
    await tester.pumpAndSettle();
    final context = tester.element(find.byType(Scaffold));
    for (final message in [
      'Phone-key capability is unavailable.',
      'Pairing is unavailable.',
      'Collect the near sample before the far sample.',
      'Collect the far sample after the near sample.',
      'Start calibration before submitting a sample.',
      'The native phone-key operation failed safely.',
      'The native phone-key service is unavailable.',
    ]) {
      expect(tr(context, message), isNot(message), reason: message);
    }
  });

  testWidgets('camera pairing flow has complete Chinese copy', (tester) async {
    tester.platformDispatcher.localesTestValue = const [Locale('zh', 'CN')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(
      ReposeUnlockApp(
        gateway: FakeNativeGateway(
          snapshot: UnlockSnapshot(
            capability: CompanionCapability.ready,
            devices: const [],
            calibration: const CalibrationSnapshot(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final context = tester.element(find.byType(Scaffold));
    for (final message in <String>[
      'Open Repose on your Mac, create a one-time pairing QR code, then scan it here.',
      'Scan Mac QR code',
      'Use camera to pair?',
      'Repose uses the camera only while this scanner is open. It reads the one-time QR code on your Mac and does not save photos.',
      'Use camera',
      'Camera access was not allowed',
      'The QR scanner stays closed. You can try again or keep using your Mac password.',
      'Allow camera access in Settings',
      'Your device will not show the camera prompt again. Open Repose permissions in Settings, allow Camera, then return to scan.',
      'Scan pairing QR code',
      'This is not a Repose pairing QR code. Scan the code shown by Repose on your Mac.',
      'Point the camera at the one-time QR code shown by Repose on your Mac.',
      'Camera access is unavailable. Return and allow it in Settings.',
      'The camera could not start. Return and try again.',
      'Connect this phone to a Repose Mac before scanning the one-time pairing QR code.',
      'Android connection saved. Next, scan the one-time pairing QR code shown on your Mac.',
      'This pairing QR code has expired.',
      'This pairing QR code has already been used.',
      'This pairing QR code has expired. Scan a new QR code.',
      'This pairing QR code was already used. Scan a new QR code.',
    ]) {
      expect(tr(context, message), isNot(message), reason: message);
      if (message.contains('pairing QR code')) {
        final localized = tr(context, message);
        expect(localized, contains('二维码'), reason: message);
        expect(localized, isNot(contains('配对码')), reason: message);
        expect(localized, isNot(contains('输入')), reason: message);
      }
    }
  });

  testWidgets('returning from guidance rechecks recovered native capability', (
    tester,
  ) async {
    final gateway = FakeNativeGateway(
      snapshot: UnlockSnapshot(
        capability: CompanionCapability.bluetoothUnavailable,
        devices: const [],
        calibration: const CalibrationSnapshot(),
      ),
    );
    await tester.pumpWidget(ReposeUnlockApp(gateway: gateway));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Permissions & background'));
    await tester.pumpAndSettle();
    gateway.snapshot = UnlockSnapshot(
      capability: CompanionCapability.ready,
      devices: const [],
      calibration: const CalibrationSnapshot(),
    );
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('READY TO PAIR'), findsOneWidget);
  });
}
