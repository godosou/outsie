import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  const gateMessage =
      'Repose Unlock release is blocked until Task 10 production signing';

  test(
    'Android release tasks fail at build time while debug stays available',
    () {
      final gradle = File('android/app/build.gradle.kts').readAsStringSync();

      expect(gradle, contains('gradle.taskGraph.whenReady'));
      expect(gradle, contains('task.name.contains("Release")'));
      expect(gradle, contains('throw GradleException(releaseGateError)'));
      expect(gradle, contains(gateMessage));
      expect(gradle, isNot(contains('task.name.contains("Debug")')));
    },
  );

  test('iOS Release and Archive builds hit an explicit compile-time gate', () {
    final releaseConfig = File(
      'ios/Flutter/Release.xcconfig',
    ).readAsStringSync();
    final debugConfig = File('ios/Flutter/Debug.xcconfig').readAsStringSync();
    final appDelegate = File('ios/Runner/AppDelegate.swift').readAsStringSync();
    final scheme = File(
      'ios/Runner.xcodeproj/xcshareddata/xcschemes/Runner.xcscheme',
    ).readAsStringSync();

    expect(releaseConfig, contains('REPOSE_TASK9_RELEASE_BLOCKED'));
    expect(debugConfig, isNot(contains('REPOSE_TASK9_RELEASE_BLOCKED')));
    expect(appDelegate, contains('#if REPOSE_TASK9_RELEASE_BLOCKED'));
    expect(appDelegate, contains('#error("$gateMessage'));
    expect(
      scheme,
      contains(RegExp(r'<ArchiveAction[\s\S]*?buildConfiguration = "Release"')),
    );
  });

  test('README documents both hard gates and the fail-closed native shell', () {
    final readme = File('README.md').readAsStringSync();

    expect(readme, contains('assembleRelease'));
    expect(readme, contains('Xcode Release/Archive'));
    expect(readme, contains('fail-closed'));
  });
}
