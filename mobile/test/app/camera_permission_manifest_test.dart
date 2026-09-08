import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('mobile manifests explain and declare camera-only QR access', () {
    final androidManifest = File(
      'android/app/src/main/AndroidManifest.xml',
    ).readAsStringSync();
    final iosInfo = File('ios/Runner/Info.plist').readAsStringSync();
    final iosPodfile = File('ios/Podfile');

    expect(
      androidManifest,
      contains('android.permission.CAMERA'),
      reason: 'Android must declare camera access for QR pairing.',
    );
    expect(iosInfo, contains('<key>NSCameraUsageDescription</key>'));
    expect(
      iosInfo,
      contains('scan the one-time pairing QR code shown on your Mac'),
    );
    expect(
      iosPodfile.existsSync(),
      isTrue,
      reason: 'CocoaPods builds must enable the permission_handler camera.',
    );
    expect(iosPodfile.readAsStringSync(), contains("'PERMISSION_CAMERA=1'"));
  });
}
