import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'package:crypto/crypto.dart';
import 'console_models.dart';

abstract interface class ConsoleTransport {
  Future<ConsoleStatus> request(Map<String, dynamic> message);
  void close();
}

bool matchesConsoleCertificate(List<int> der, String fingerprint) =>
    sha256.convert(der).toString() == fingerprint;

class ConsoleClient implements ConsoleTransport {
  ConsoleClient(this.pairing)
    : _http = HttpClient(context: SecurityContext(withTrustedRoots: false)) {
    _http.connectionTimeout = const Duration(seconds: 2);
    _http.findProxy = (_) => 'DIRECT';
    _http.badCertificateCallback = (certificate, host, port) =>
        host == pairing.host &&
        port == pairing.port &&
        matchesConsoleCertificate(certificate.der, pairing.fingerprint);
  }
  final ConsolePairing pairing;
  final HttpClient _http;
  bool _closed = false;

  @override
  Future<ConsoleStatus> request(Map<String, dynamic> message) async {
    if (_closed) throw const SocketException('Console disconnected');
    try {
      return await _send(message).timeout(const Duration(seconds: 2));
    } on ConsoleRequestRejected {
      rethrow;
    } catch (_) {
      close(); // Aborts a timed out request; it must never be replayed.
      rethrow;
    }
  }

  Future<ConsoleStatus> _send(Map<String, dynamic> message) async {
    final request = await _http.postUrl(
      Uri(
        scheme: 'https',
        host: pairing.host,
        port: pairing.port,
        path: '/console',
      ),
    );
    request.followRedirects = false;
    request.headers.contentType = ContentType.json;
    request.headers.set(
      HttpHeaders.authorizationHeader,
      'Bearer ${pairing.token}',
    );
    final payload = utf8.encode(jsonEncode(message));
    request.contentLength = payload.length;
    request.add(payload);
    final response = await request.close();
    final certificate = response.certificate;
    if (certificate == null ||
        !matchesConsoleCertificate(certificate.der, pairing.fingerprint)) {
      throw const HandshakeException('Console certificate mismatch');
    }
    final bytes = <int>[];
    await for (final chunk in response) {
      bytes.addAll(chunk);
      if (bytes.length > 262144) {
        throw const FormatException('Console response too large');
      }
    }
    final body = jsonDecode(utf8.decode(bytes));
    if (body is Map<String, dynamic> &&
        body['ok'] == false &&
        response.statusCode == 400) {
      throw ConsoleRequestRejected();
    }
    if (response.statusCode != 200 || body is! Map<String, dynamic>) {
      throw const HttpException('Console request rejected');
    }
    if (body['ok'] != true) throw ConsoleRequestRejected();
    if (body['data'] is! Map<String, dynamic>) {
      throw const FormatException('Invalid console response');
    }
    return ConsoleStatus.fromJson(body['data'] as Map<String, dynamic>);
  }

  @override
  void close() {
    _closed = true;
    _http.close(force: true);
  }
}

class ConsoleRequestRejected implements Exception {}

String consoleRequestId() {
  final random = Random.secure();
  return base64UrlEncode(
    List<int>.generate(18, (_) => random.nextInt(256)),
  ).replaceAll('=', '');
}
