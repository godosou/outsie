import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:permission_handler/permission_handler.dart';

import '../../app/app_text.dart';

const pairingQrPrefix = 'repose://pair/v1/';
const maxPairingQrLength = 4096;

final _base64UrlPayload = RegExp(r'^[A-Za-z0-9_-]+$');

bool isReposePairingQrPayload(String? value) {
  if (value == null ||
      value.length > maxPairingQrLength ||
      !value.startsWith(pairingQrPrefix)) {
    return false;
  }
  return _base64UrlPayload.hasMatch(value.substring(pairingQrPrefix.length));
}

enum CameraAccessOutcome { granted, denied, permanentlyDenied }

abstract interface class PairingCameraAccess {
  Future<CameraAccessOutcome> request();

  Future<bool> openSettings();
}

class SystemPairingCameraAccess implements PairingCameraAccess {
  const SystemPairingCameraAccess();

  @override
  Future<CameraAccessOutcome> request() async {
    final status = await Permission.camera.request();
    if (status.isGranted) {
      return CameraAccessOutcome.granted;
    }
    if (status.isPermanentlyDenied || status.isRestricted) {
      return CameraAccessOutcome.permanentlyDenied;
    }
    return CameraAccessOutcome.denied;
  }

  @override
  Future<bool> openSettings() => openAppSettings();
}

typedef PairingScannerBuilder =
    Widget Function(BuildContext context, ValueChanged<String?> onDetected);

class PairingScannerPage extends StatefulWidget {
  const PairingScannerPage({this.scannerBuilder, super.key});

  final PairingScannerBuilder? scannerBuilder;

  @override
  State<PairingScannerPage> createState() => _PairingScannerPageState();
}

class _PairingScannerPageState extends State<PairingScannerPage> {
  bool _completed = false;
  bool _showInvalidCode = false;

  void _handleDetected(String? value) {
    if (_completed) {
      return;
    }
    if (!isReposePairingQrPayload(value)) {
      if (!_showInvalidCode && mounted) {
        setState(() => _showInvalidCode = true);
      }
      return;
    }
    _completed = true;
    Navigator.of(context).pop(value);
  }

  @override
  Widget build(BuildContext context) {
    final scanner =
        widget.scannerBuilder?.call(context, _handleDetected) ??
        _MobilePairingScanner(onDetected: _handleDetected);
    return Scaffold(
      key: const Key('pairingScannerPage'),
      backgroundColor: Colors.black,
      appBar: AppBar(
        foregroundColor: Colors.white,
        backgroundColor: Colors.black,
        title: const AppText('Scan pairing QR code'),
      ),
      body: Stack(
        fit: StackFit.expand,
        children: <Widget>[
          scanner,
          const IgnorePointer(child: _ScannerFrame()),
          Align(
            alignment: Alignment.bottomCenter,
            child: SafeArea(
              minimum: const EdgeInsets.fromLTRB(20, 20, 20, 28),
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: const Color(0xe6202b24),
                  borderRadius: BorderRadius.circular(18),
                  border: Border.all(color: const Color(0x665f7851)),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: AnimatedSwitcher(
                    duration: const Duration(milliseconds: 180),
                    child: _showInvalidCode
                        ? const AppText(
                            'This is not a Repose pairing QR code. Scan the code shown by Repose on your Mac.',
                            key: Key('invalidPairingQrMessage'),
                            textAlign: TextAlign.center,
                            style: TextStyle(color: Colors.white),
                          )
                        : const AppText(
                            'Point the camera at the one-time QR code shown by Repose on your Mac.',
                            textAlign: TextAlign.center,
                            style: TextStyle(color: Colors.white),
                          ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _MobilePairingScanner extends StatelessWidget {
  const _MobilePairingScanner({required this.onDetected});

  final ValueChanged<String?> onDetected;

  @override
  Widget build(BuildContext context) {
    return MobileScanner(
      tapToFocus: true,
      onDetect: (capture) {
        for (final barcode in capture.barcodes) {
          final value = barcode.rawValue;
          if (value != null) {
            onDetected(value);
            return;
          }
        }
      },
      errorBuilder: (context, error) => ColoredBox(
        color: Colors.black,
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(28),
            child: AppText(
              error.errorCode == MobileScannerErrorCode.permissionDenied
                  ? 'Camera access is unavailable. Return and allow it in Settings.'
                  : 'The camera could not start. Return and try again.',
              textAlign: TextAlign.center,
              style: const TextStyle(color: Colors.white),
            ),
          ),
        ),
      ),
    );
  }
}

class _ScannerFrame extends StatelessWidget {
  const _ScannerFrame();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: FractionallySizedBox(
        widthFactor: 0.72,
        child: AspectRatio(
          aspectRatio: 1,
          child: DecoratedBox(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(28),
              border: Border.all(color: const Color(0xff9fbd8c), width: 3),
              boxShadow: const <BoxShadow>[
                BoxShadow(
                  color: Color(0x88526a43),
                  blurRadius: 24,
                  spreadRadius: 2,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
