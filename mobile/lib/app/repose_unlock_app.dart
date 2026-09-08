import 'dart:async';

import 'package:flutter/material.dart';

import '../features/calibration/calibration_controller.dart';
import '../features/devices/device_controller.dart';
import '../features/pairing/pairing_controller.dart';
import '../native/native_gateway.dart';
import '../native/native_models.dart';

class ReposeUnlockApp extends StatelessWidget {
  const ReposeUnlockApp({required this.gateway, super.key});

  final NativeGateway gateway;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Repose Phone Key',
      theme: ThemeData(
        brightness: Brightness.dark,
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xff5eead4),
          brightness: Brightness.dark,
          surface: const Color(0xff111c2e),
        ),
        scaffoldBackgroundColor: const Color(0xff08111f),
        appBarTheme: const AppBarTheme(
          backgroundColor: Colors.transparent,
          foregroundColor: Color(0xfff8fafc),
          elevation: 0,
          centerTitle: false,
        ),
        cardTheme: CardThemeData(
          color: const Color(0xff111c2e),
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(20),
            side: const BorderSide(color: Color(0xff26364d)),
          ),
        ),
        inputDecorationTheme: InputDecorationTheme(
          filled: true,
          fillColor: const Color(0xff0c1727),
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(16),
            borderSide: const BorderSide(color: Color(0xff31445e)),
          ),
          enabledBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(16),
            borderSide: const BorderSide(color: Color(0xff31445e)),
          ),
        ),
        filledButtonTheme: FilledButtonThemeData(
          style: FilledButton.styleFrom(
            minimumSize: const Size.fromHeight(52),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(16),
            ),
          ),
        ),
        useMaterial3: true,
      ),
      home: _CompanionHome(gateway: gateway),
    );
  }
}

class _CompanionHome extends StatefulWidget {
  const _CompanionHome({required this.gateway});

  final NativeGateway gateway;

  @override
  State<_CompanionHome> createState() => _CompanionHomeState();
}

class _CompanionHomeState extends State<_CompanionHome> {
  late final DeviceController _devices;
  late final PairingController _pairing;
  late final CalibrationController _calibration;
  late final Listenable _controllers;
  final _qrController = TextEditingController();
  var _showAdditionalPairing = false;

  @override
  void initState() {
    super.initState();
    _devices = DeviceController(gateway: widget.gateway);
    _pairing = PairingController(
      gateway: widget.gateway,
      deviceController: _devices,
    );
    _calibration = CalibrationController(
      gateway: widget.gateway,
      deviceController: _devices,
    );
    _controllers = Listenable.merge(<Listenable>[
      _devices,
      _pairing,
      _calibration,
    ]);
    _devices.addListener(_syncCalibrationFromAuthoritativeSnapshot);
    unawaited(_hydrateFromNative());
  }

  Future<void> _hydrateFromNative() async {
    final hydrated = await _devices.hydrate();
    if (!hydrated || !mounted) {
      return;
    }
    final snapshot = _devices.state.snapshot;
    if (snapshot == null) {
      return;
    }
    _pairing.hydrateFromSnapshot(snapshot.pendingPairing);
  }

  void _syncCalibrationFromAuthoritativeSnapshot() {
    final deviceState = _devices.state;
    final snapshot = deviceState.snapshot;
    if (!deviceState.isHydrated ||
        deviceState.isMutationBusy ||
        snapshot == null) {
      return;
    }
    _calibration.hydrateFromSnapshot(snapshot.calibration);
  }

  @override
  void dispose() {
    _devices.removeListener(_syncCalibrationFromAuthoritativeSnapshot);
    _qrController.dispose();
    _calibration.dispose();
    _pairing.dispose();
    _devices.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text(
          'Repose Key',
          style: TextStyle(fontWeight: FontWeight.w700, letterSpacing: -0.4),
        ),
        actions: const <Widget>[
          Padding(
            padding: EdgeInsets.only(right: 20),
            child: Icon(Icons.shield_outlined, size: 22),
          ),
        ],
      ),
      body: AnimatedBuilder(
        animation: _controllers,
        builder: (context, _) {
          final deviceState = _devices.state;
          final showPrimaryPairing =
              deviceState.devices.isEmpty ||
              (!_showAdditionalPairing &&
                  _pairing.state.phase != PairingPhase.idle);
          return SafeArea(
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 720),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: <Widget>[
                      _PhoneKeyHero(
                        state: deviceState,
                        calibration: _calibration.state,
                      ),
                      const SizedBox(height: 12),
                      if (showPrimaryPairing) ...[
                        _PairingCard(
                          gate: deviceState.gate,
                          controller: _pairing,
                          qrController: _qrController,
                        ),
                        const SizedBox(height: 12),
                      ],
                      if (deviceState.devices.isNotEmpty) ...[
                        _CalibrationCard(
                          gate: deviceState.gate,
                          hasPairedDevice: true,
                          controller: _calibration,
                        ),
                        const SizedBox(height: 12),
                      ],
                      _SetupProgress(
                        state: deviceState,
                        calibration: _calibration.state,
                      ),
                      const SizedBox(height: 12),
                      if (deviceState.devices.isEmpty) ...[
                        _CalibrationCard(
                          gate: deviceState.gate,
                          hasPairedDevice: false,
                          controller: _calibration,
                        ),
                        const SizedBox(height: 12),
                      ],
                      _DevicesCard(
                        controller: _devices,
                        onAddDevice:
                            deviceState.devices.isNotEmpty &&
                                !_showAdditionalPairing
                            ? () =>
                                  setState(() => _showAdditionalPairing = true)
                            : null,
                      ),
                      if (deviceState.devices.isNotEmpty &&
                          _showAdditionalPairing) ...<Widget>[
                        const SizedBox(height: 12),
                        _PairingCard(
                          gate: deviceState.gate,
                          controller: _pairing,
                          qrController: _qrController,
                        ),
                      ],
                      const SizedBox(height: 12),
                      const Text(
                        'Password sign-in remains available if phone key is '
                        'unavailable.',
                        textAlign: TextAlign.center,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}

class _PhoneKeyHero extends StatelessWidget {
  const _PhoneKeyHero({required this.state, required this.calibration});

  final DeviceState state;
  final CalibrationState calibration;

  @override
  Widget build(BuildContext context) {
    final gate = state.gate;
    final hasPairedDevice = state.devices.isNotEmpty;
    final isCalibrated = calibration.phase == CalibrationPhase.complete;
    final isReady = gate.canPair && hasPairedDevice && isCalibrated;
    final (statusLabel, headline, detail) = switch ((
      gate.canPair,
      hasPairedDevice,
      isCalibrated,
    )) {
      (false, _, _) => (
        'PHONE KEY OFFLINE',
        'Phone key unavailable',
        gate.message ?? 'Phone key setup is paused.',
      ),
      (true, false, _) => (
        'READY TO PAIR',
        'Add your phone key',
        'Pair this phone with your Mac to continue.',
      ),
      (true, true, false) => (
        'CALIBRATION NEEDED',
        'Set your unlock distance',
        'Choose where your Mac should recognize this phone.',
      ),
      (true, true, true) => (
        'SETUP COMPLETE',
        'Phone key setup complete',
        'Secure setup is complete on this device.',
      ),
    };
    final accent = isReady ? const Color(0xff6ee7b7) : const Color(0xffffc857);
    final capabilityStatus = gate.canPair
        ? 'Phone key setup is ready'
        : state.message ?? gate.message ?? 'Phone key is unavailable.';

    return Container(
      key: const Key('phoneKeyHero'),
      padding: const EdgeInsets.fromLTRB(20, 18, 20, 18),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(28),
        gradient: const LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: <Color>[Color(0xff15263d), Color(0xff0a111d)],
        ),
        boxShadow: const <BoxShadow>[
          BoxShadow(
            color: Color(0x29000000),
            blurRadius: 28,
            offset: Offset(0, 14),
          ),
        ],
      ),
      child: Column(
        children: <Widget>[
          Align(
            alignment: Alignment.centerLeft,
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              decoration: BoxDecoration(
                color: accent.withValues(alpha: 0.12),
                borderRadius: BorderRadius.circular(999),
                border: Border.all(color: accent.withValues(alpha: 0.38)),
              ),
              child: Text(
                statusLabel,
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: accent,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 1.25,
                ),
              ),
            ),
          ),
          const SizedBox(height: 10),
          Container(
            key: const Key('proximityOrb'),
            width: 96,
            height: 96,
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: accent.withValues(alpha: 0.06),
              border: Border.all(color: accent.withValues(alpha: 0.16)),
              boxShadow: <BoxShadow>[
                BoxShadow(
                  color: accent.withValues(alpha: 0.12),
                  blurRadius: 32,
                  spreadRadius: 8,
                ),
              ],
            ),
            child: Container(
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: accent.withValues(alpha: 0.12),
                border: Border.all(color: accent.withValues(alpha: 0.46)),
              ),
              child: Icon(Icons.key_rounded, size: 34, color: accent),
            ),
          ),
          const SizedBox(height: 10),
          Text(
            hasPairedDevice ? state.devices.first.displayName : 'THIS PHONE',
            style: Theme.of(context).textTheme.labelMedium?.copyWith(
              color: const Color(0xff8fa4bb),
              fontWeight: FontWeight.w700,
              letterSpacing: 1.1,
            ),
          ),
          const SizedBox(height: 5),
          Text(
            headline,
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.headlineSmall?.copyWith(
              color: Colors.white,
              fontWeight: FontWeight.w700,
            ),
          ),
          const SizedBox(height: 6),
          Semantics(
            container: true,
            liveRegion: true,
            label: 'Phone key status\n$capabilityStatus',
            child: ExcludeSemantics(
              child: Column(
                children: <Widget>[
                  Text(
                    detail,
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      color: const Color(0xffaebed0),
                      height: 1.4,
                    ),
                  ),
                  if (capabilityStatus != detail) ...<Widget>[
                    const SizedBox(height: 10),
                    Text(
                      capabilityStatus,
                      textAlign: TextAlign.center,
                      style: const TextStyle(color: Color(0xff8fa4bb)),
                    ),
                  ],
                ],
              ),
            ),
          ),
          if (gate.isPermanentlyUnsupported) ...<Widget>[
            const SizedBox(height: 10),
            const Text(
              'No background polling fallback will be enabled.',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: Color(0xffffc857),
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
          if (gate.canPair) ...<Widget>[
            const SizedBox(height: 8),
            const Row(
              mainAxisAlignment: MainAxisAlignment.center,
              children: <Widget>[
                Icon(Icons.shield_outlined, size: 16, color: Color(0xff8fa4bb)),
                SizedBox(width: 7),
                Flexible(
                  child: Text(
                    'Protected by on-device security',
                    textAlign: TextAlign.center,
                    style: TextStyle(color: Color(0xff8fa4bb)),
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

class _SetupProgress extends StatelessWidget {
  const _SetupProgress({required this.state, required this.calibration});

  final DeviceState state;
  final CalibrationState calibration;

  @override
  Widget build(BuildContext context) {
    final gate = state.gate;
    return Container(
      key: const Key('setupProgress'),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 16),
      decoration: BoxDecoration(
        color: const Color(0xff0f1a2a),
        borderRadius: BorderRadius.circular(20),
        border: Border.all(color: const Color(0xff26364d)),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Expanded(
            child: _ProgressStep(
              icon: Icons.shield_outlined,
              label: 'Secure',
              isComplete: gate.canPair,
            ),
          ),
          const _ProgressConnector(),
          Expanded(
            child: _ProgressStep(
              icon: Icons.link_rounded,
              label: 'Paired',
              isComplete: state.devices.isNotEmpty,
            ),
          ),
          const _ProgressConnector(),
          Expanded(
            child: _ProgressStep(
              icon: Icons.radar_rounded,
              label: 'Distance',
              isComplete: calibration.phase == CalibrationPhase.complete,
            ),
          ),
        ],
      ),
    );
  }
}

class _ProgressStep extends StatelessWidget {
  const _ProgressStep({
    required this.icon,
    required this.label,
    required this.isComplete,
  });

  final IconData icon;
  final String label;
  final bool isComplete;

  @override
  Widget build(BuildContext context) {
    final color = isComplete
        ? const Color(0xff5eead4)
        : const Color(0xff71839a);
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: <Widget>[
        Icon(isComplete ? Icons.check_circle_rounded : icon, color: color),
        const SizedBox(height: 7),
        Text(
          label,
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.labelMedium?.copyWith(
            color: color,
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
    );
  }
}

class _ProgressConnector extends StatelessWidget {
  const _ProgressConnector();

  @override
  Widget build(BuildContext context) {
    return const Expanded(
      child: Padding(
        padding: EdgeInsets.only(top: 11),
        child: Divider(color: Color(0xff31445e), height: 1),
      ),
    );
  }
}

class _PairingCard extends StatelessWidget {
  const _PairingCard({
    required this.gate,
    required this.controller,
    required this.qrController,
  });

  final CapabilityGate gate;
  final PairingController controller;
  final TextEditingController qrController;

  bool get _busy => controller.state.isBusy;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    return _SectionCard(
      icon: Icons.qr_code_scanner,
      title: 'Pair this phone',
      children: <Widget>[
        const Text('Paste the one-time pairing code shown on your Mac.'),
        const SizedBox(height: 10),
        TextField(
          key: const Key('pairingQrField'),
          controller: qrController,
          enabled: gate.canPair && !_busy,
          autocorrect: false,
          enableSuggestions: false,
          decoration: const InputDecoration(
            border: OutlineInputBorder(),
            labelText: 'One-time pairing code',
          ),
        ),
        const SizedBox(height: 8),
        FilledButton.icon(
          key: const Key('beginPairingButton'),
          onPressed: gate.canPair && !_busy
              ? () {
                  final payload = qrController.text;
                  qrController.clear();
                  unawaited(controller.beginPairing(payload));
                }
              : null,
          icon: const Icon(Icons.link),
          label: const Text('Begin pairing'),
        ),
        if (state.phase == PairingPhase.awaitingConfirmation) ...<Widget>[
          const SizedBox(height: 10),
          _LiveStatus(
            'Confirm ${state.deviceName} before pairing',
            style: Theme.of(context).textTheme.titleSmall,
          ),
          const SizedBox(height: 6),
          FilledButton(
            key: const Key('confirmPairingButton'),
            onPressed: gate.canPair && !_busy
                ? () => unawaited(controller.confirmPairing())
                : null,
            child: const Text('Confirm this device'),
          ),
        ],
        if (state.phase == PairingPhase.paired) ...<Widget>[
          const SizedBox(height: 8),
          const _LiveStatus('Pairing confirmed'),
        ] else if (state.message != null) ...<Widget>[
          const SizedBox(height: 8),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }
}

class _CalibrationCard extends StatelessWidget {
  const _CalibrationCard({
    required this.gate,
    required this.hasPairedDevice,
    required this.controller,
  });

  final CapabilityGate gate;
  final bool hasPairedDevice;
  final CalibrationController controller;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    final canMutate = gate.canCalibrate && hasPairedDevice && !state.isBusy;
    return _SectionCard(
      icon: Icons.social_distance,
      title: 'Distance calibration',
      children: <Widget>[
        const Text('Collect near for about 8 seconds, then far for 8 seconds.'),
        if (!hasPairedDevice) ...<Widget>[
          const SizedBox(height: 6),
          const Text('Pair a phone before calibration.'),
        ],
        const SizedBox(height: 8),
        FilledButton.icon(
          key: const Key('startCalibrationButton'),
          onPressed: canMutate
              ? () => unawaited(controller.startCalibration())
              : null,
          icon: const Icon(Icons.tune),
          label: Text(
            state.canRetry ? 'Retry calibration' : 'Start calibration',
          ),
        ),
        if (state.phase == CalibrationPhase.collectingNear) ...<Widget>[
          const SizedBox(height: 8),
          FilledButton(
            key: const Key('submitNearButton'),
            onPressed: canMutate
                ? () => unawaited(controller.submitStep(CalibrationStep.near))
                : null,
            child: const Text('Finish near sample'),
          ),
        ],
        if (state.phase == CalibrationPhase.collectingFar) ...<Widget>[
          const SizedBox(height: 8),
          FilledButton(
            key: const Key('submitFarButton'),
            onPressed: canMutate
                ? () => unawaited(controller.submitStep(CalibrationStep.far))
                : null,
            child: const Text('Finish far sample'),
          ),
        ],
        if (state.message != null) ...<Widget>[
          const SizedBox(height: 8),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }
}

class _DevicesCard extends StatelessWidget {
  const _DevicesCard({required this.controller, this.onAddDevice});

  final DeviceController controller;
  final VoidCallback? onAddDevice;

  @override
  Widget build(BuildContext context) {
    final state = controller.state;
    return _SectionCard(
      icon: Icons.devices,
      title: 'Paired devices',
      children: <Widget>[
        if (state.devices.isEmpty)
          const Text('No paired devices yet.')
        else
          for (final device in state.devices)
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: Text(device.displayName),
              subtitle: Text(
                device.platform == CompanionPlatform.android
                    ? 'Android'
                    : 'iOS',
              ),
              trailing: IconButton(
                tooltip: 'Revoke ${device.displayName}',
                onPressed: state.isMutationBusy
                    ? null
                    : () => _confirmRevocation(context, device),
                icon: const Icon(Icons.link_off),
              ),
            ),
        if (state.devices.isNotEmpty && onAddDevice != null) ...<Widget>[
          const SizedBox(height: 4),
          OutlinedButton.icon(
            key: const Key('addAnotherKeyButton'),
            onPressed: onAddDevice,
            icon: const Icon(Icons.add_rounded),
            label: const Text('Add another key'),
          ),
        ],
        if (state.message != null) ...<Widget>[
          const SizedBox(height: 6),
          _LiveStatus(state.message!),
        ],
      ],
    );
  }

  Future<void> _confirmRevocation(
    BuildContext context,
    PairedDevice device,
  ) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Revoke phone key?'),
        content: Text('${device.displayName} will need to pair again.'),
        actions: <Widget>[
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Revoke'),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      await controller.revokeDevice(device.id);
    }
  }
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({
    required this.icon,
    required this.title,
    required this.children,
  });

  final IconData icon;
  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            Row(
              children: <Widget>[
                Icon(icon),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    title,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            ...children,
          ],
        ),
      ),
    );
  }
}

class _LiveStatus extends StatelessWidget {
  const _LiveStatus(this.message, {this.style});

  final String message;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    return Semantics(liveRegion: true, child: Text(message, style: style));
  }
}
