import 'package:flutter_test/flutter_test.dart';
import 'package:repose_unlock/features/pairing/pairing_scanner.dart';

void main() {
  test('only canonical Repose v1 pairing QR payloads are accepted', () {
    expect(isReposePairingQrPayload('repose://pair/v1/opaque'), isTrue);
    expect(isReposePairingQrPayload('repose://pair/v1/'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/opaque'), isFalse);
    expect(isReposePairingQrPayload('REPOSE://pair/v1/opaque'), isFalse);
    expect(isReposePairingQrPayload(' repose://pair/v1/opaque'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/v1/op aque'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/v1/opaque?copy=1'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/v1/opaque#copy'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/v1/opaque/extra'), isFalse);
    expect(isReposePairingQrPayload('repose://pair/v1/opaque='), isFalse);
    expect(
      isReposePairingQrPayload(
        '$pairingQrPrefix${List<String>.filled(4096 - pairingQrPrefix.length, 'A').join()}',
      ),
      isTrue,
    );
    expect(
      isReposePairingQrPayload(
        '$pairingQrPrefix${List<String>.filled(4097 - pairingQrPrefix.length, 'A').join()}',
      ),
      isFalse,
    );
    expect(isReposePairingQrPayload(null), isFalse);
  });
}
