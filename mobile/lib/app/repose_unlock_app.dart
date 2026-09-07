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
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xff375a7f)),
        scaffoldBackgroundColor: const Color(0xfff5f7fa),
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
    _calibration.hydrateFromSnapshot(snapshot.calibration);
  }

  @override
  void dispose() {
    _qrController.dispose();
    _calibration.dispose();
    _pairing.dispose();
    _devices.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Repose Phone Key')),
      body: AnimatedBuilder(
        animation: _controllers,
        builder: (context, _) {
          final deviceState = _devices.state;
          return SafeArea(
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 720),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: <Widget>[
                      const _TargetCard(),
                      const SizedBox(height: 12),
                      _CapabilityCard(state: deviceState),
                      const SizedBox(height: 12),
                      _PairingCard(
                        gate: deviceState.gate,
                        controller: _pairing,
                        qrController: _qrController,
                      ),
                      const SizedBox(height: 12),
                      _CalibrationCard(
                        gate: deviceState.gate,
                        hasPairedDevice: deviceState.devices.isNotEmpty,
                        controller: _calibration,
                      ),
                      const SizedBox(height: 12),
                      _DevicesCard(controller: _devices),
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

class _TargetCard extends StatelessWidget {
  const _TargetCard();

  @override
  Widget build(BuildContext context) {
    return const _SectionCard(
      icon: Icons.phone_android,
      title: 'Companion target',
      children: <Widget>[
        Text('Initial Android target: realme GT5 Pro · Android 16 (API 36)'),
        SizedBox(height: 4),
        Text('One shared companion UI for Android and iOS.'),
      ],
    );
  }
}

class _CapabilityCard extends StatelessWidget {
  const _CapabilityCard({required this.state});

  final DeviceState state;

  @override
  Widget build(BuildContext context) {
    final gate = state.gate;
    final status = gate.canPair
        ? 'Native capabilities ready'
        : state.message ?? gate.message ?? 'Phone key is unavailable.';
    return _SectionCard(
      icon: gate.canPair ? Icons.verified_user : Icons.shield_outlined,
      title: 'Capability status',
      trailing: state.isLoading
          ? const SizedBox.square(
              dimension: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          : null,
      children: <Widget>[
        _LiveStatus(status),
        if (gate.isPermanentlyUnsupported) ...<Widget>[
          const SizedBox(height: 6),
          const Text('No background polling fallback will be enabled.'),
        ],
      ],
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
        const Text(
          'Scan or paste the one-time Mac QR payload. Repose checks expiry '
          'again in the native service.',
        ),
        const SizedBox(height: 10),
        TextField(
          key: const Key('pairingQrField'),
          controller: qrController,
          enabled: gate.canPair && !_busy,
          autocorrect: false,
          enableSuggestions: false,
          decoration: const InputDecoration(
            border: OutlineInputBorder(),
            labelText: 'One-time pairing QR payload',
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
  const _DevicesCard({required this.controller});

  final DeviceController controller;

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
    this.trailing,
  });

  final IconData icon;
  final String title;
  final List<Widget> children;
  final Widget? trailing;

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
                ?trailing,
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
