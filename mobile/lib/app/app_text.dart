import 'package:flutter/material.dart';

String tr(BuildContext context, String value) {
  if (Localizations.localeOf(context).languageCode != 'zh') return value;
  final translated = _zh[value];
  if (translated != null) return translated;
  if (value.startsWith('Confirm ') && value.endsWith(' before pairing')) {
    return '请确认设备：${value.substring(8, value.length - 15)}';
  }
  if (value.startsWith('Revoke ')) return '移除 ${value.substring(7)}';
  if (value.endsWith(' will need to pair again.')) {
    return '${value.substring(0, value.length - 25)} 需要重新配对后才能使用。';
  }
  return value;
}

class AppText extends StatelessWidget {
  const AppText(
    this.data, {
    super.key,
    this.style,
    this.textAlign,
    this.maxLines,
    this.overflow,
  });
  final String data;
  final TextStyle? style;
  final TextAlign? textAlign;
  final int? maxLines;
  final TextOverflow? overflow;
  @override
  Widget build(BuildContext context) => Text(
    tr(context, data),
    style: style,
    textAlign: textAlign,
    maxLines: maxLines,
    overflow: overflow,
  );
}

const _zh = <String, String>{
  'Phone key': '手机钥匙',
  'Repose Key': '手机钥匙',
  'Permissions & background': '权限与后台运行',
  'A LITTLE CLOSER, A LITTLE CALMER': '靠近一点，从容继续',
  'PHONE KEY OFFLINE': '暂未启用',
  'CHECKING': '正在检查',
  'Checking your phone key': '正在检查手机钥匙',
  'This will only take a moment.': '请稍候，很快就好。',
  'CONNECT YOUR MAC': '连接你的 MAC',
  'Connect Android to your Mac': '先让手机连接你的 Mac',
  'Allow Android to discover the Repose service before secure pairing.':
      '先允许 Android 查找 Repose Mac，再进行安全配对。',
  'Phone key unavailable': '手机钥匙暂不可用',
  'Phone key setup is paused.': '手机钥匙设置已暂停。',
  'READY TO PAIR': '等待配对',
  'Add your phone key': '让手机，成为你的钥匙',
  'Pair this phone with your Mac to continue.': '连接你的 Mac，开始设置手机钥匙。',
  'CALIBRATION NEEDED': '等待距离校准',
  'Set your unlock distance': '找到刚刚好的距离',
  'Choose where your Mac should recognize this phone.': '告诉 Mac，你希望在哪个位置被识别。',
  'SETUP COMPLETE': '设置已完成',
  'Phone key setup complete': '手机钥匙设置完成',
  'Secure setup is complete on this device.': '这台设备的配对与距离校准已完成。',
  'Phone key setup is ready': '可以开始设置手机钥匙',
  'Phone key is unavailable.': '手机钥匙暂不可用。',
  'Phone key status': '手机钥匙状态',
  'THIS PHONE': '这台手机',
  'Protected by on-device security': '密钥由设备安全硬件保护',
  'Connect this phone': '连接这台手机',
  'Let Android find the Repose service advertised by your Mac. This system connection does not unlock your Mac; secure pairing comes next.':
      '先让 Android 查找 Mac 上的 Repose 蓝牙服务。这里只建立系统连接，不会直接解锁 Mac；下一步还会进行安全配对。',
  'Find a Repose Mac': '查找 Repose Mac',
  'You can keep signing in with your Mac password.':
      '此平台暂不支持自动靠近解锁，仍可使用 Mac 密码登录。',
  'Secure': '安全检查',
  'Paired': '设备配对',
  'Distance': '距离校准',
  'Pair this phone': '连接你的 Mac',
  'Open Repose on your Mac, create a one-time pairing QR code, then scan it here.':
      '打开 Mac 上的 Repose，生成一次性配对二维码，然后用手机扫描。',
  'Scan Mac QR code': '扫描 Mac 二维码',
  'Use camera to pair?': '使用相机完成配对？',
  'Repose uses the camera only while this scanner is open. It reads the one-time QR code on your Mac and does not save photos.':
      'Repose 只会在扫码页面打开相机，用于读取 Mac 上的一次性二维码，不会保存照片。',
  'Use camera': '使用相机',
  'Camera access was not allowed': '未允许相机访问',
  'The QR scanner stays closed. You can try again or keep using your Mac password.':
      '二维码扫描器不会启动。你可以重新尝试，也可以继续使用 Mac 密码。',
  'Allow camera access in Settings': '请在系统设置中允许相机',
  'Your device will not show the camera prompt again. Open Repose permissions in Settings, allow Camera, then return to scan.':
      '系统不会再次弹出相机授权。请打开 Repose 的系统权限，允许相机后返回扫码。',
  'Scan pairing QR code': '扫描配对二维码',
  'This is not a Repose pairing QR code. Scan the code shown by Repose on your Mac.':
      '这不是 Repose 配对二维码。请扫描 Mac 上 Repose 显示的二维码。',
  'Point the camera at the one-time QR code shown by Repose on your Mac.':
      '将相机对准 Mac 上 Repose 显示的一次性二维码。',
  'Camera access is unavailable. Return and allow it in Settings.':
      '相机权限不可用。请返回并在系统设置中允许相机。',
  'The camera could not start. Return and try again.': '相机无法启动，请返回后重试。',
  'Confirm this device': '确认配对',
  'Pairing confirmed': '配对成功',
  'Phone-key capability is unavailable.': '手机钥匙暂不可用，请稍后重试。',
  'Pairing is unavailable.': '暂时无法配对，请稍后重试。',
  'Collect the near sample before the far sample.': '请先完成近距离采集，再进行远距离采集。',
  'Collect the far sample after the near sample.': '近距离采集已完成，请继续采集远距离信号。',
  'Start calibration before submitting a sample.': '请先开始校准，再采集信号。',
  'This pairing QR code has expired.': '配对二维码已过期，请在 Mac 上生成新的二维码。',
  'This pairing QR code has already been used.': '配对二维码已使用，请在 Mac 上生成新的二维码。',
  'This phone-key capability is unavailable.': '手机钥匙暂不可用。',
  'This phone-key operation is not supported on this device.': '这台设备暂不支持此操作。',
  'Calibration steps must be completed in order.': '请按照先近后远的顺序完成校准。',
  'The near and far calibration samples overlap. Try again.':
      '两个位置的信号太接近，请拉开距离后重试。',
  'The paired device was not found.': '未找到已配对的设备，请刷新后重试。',
  'The native phone-key service is unavailable.': '手机钥匙服务暂不可用，请稍后重试。',
  'A companion association request is already open.': '系统连接窗口已经打开。',
  'Nearby devices access is required to find your Mac.':
      '需要允许「附近设备」权限，才能查找你的 Mac。',
  'No Mac was associated. You can try again.': '尚未连接 Mac，可以重新尝试。',
  'Open Repose on your phone before starting system association.':
      '请先在手机上打开 Repose，再开始系统连接。',
  'Repose could not find a Mac advertising the phone-key service.':
      '没有找到正在广播手机钥匙服务的 Repose Mac。',
  'Android did not confirm the companion association.':
      'Android 尚未确认这次系统连接，请重试。',
  'The native phone-key operation failed safely.': '操作未完成，手机钥匙状态未获确认。请稍后重试。',
  'Distance calibration': '距离校准',
  'Collect near for about 8 seconds, then far for 8 seconds.':
      '先在常用座位旁采集约 8 秒，再走到希望被视为离开的位置采集约 8 秒。',
  'Pair a phone before calibration.': '完成设备配对后，就可以设置距离。',
  'Retry calibration': '重新校准',
  'Start calibration': '开始校准',
  'Finish near sample': '完成近距离采集',
  'Finish far sample': '完成远距离采集',
  'Paired devices': '已配对的设备',
  'No paired devices yet.': '还没有配对设备。完成配对后，会在这里显示。',
  'Add another key': '添加另一台设备',
  'Revoke phone key?': '移除这把手机钥匙？',
  'Cancel': '取消',
  'Revoke': '移除钥匙',
  'Password sign-in remains available if phone key is unavailable.':
      '暂时用不了手机钥匙？仍可照常使用 Mac 密码登录。',
  'Checking native phone-key capabilities…': '正在检查手机钥匙状态…',
  'Connect this phone to a Repose Mac before scanning the one-time pairing QR code.':
      '请先连接这台手机与 Repose Mac，再扫描一次性配对二维码。',
  'Bluetooth is unavailable. Turn it on to continue.': '蓝牙暂不可用，请检查蓝牙开关和附近设备权限。',
  'Secure hardware keys are unavailable on this device.': '此设备暂不支持手机钥匙所需的安全密钥。',
  'Phone key is not enabled in this version. Changing background settings alone will not enable it.':
      '当前版本尚未开放手机钥匙功能。仅修改后台设置，还不能启用自动解锁。',
  'Automatic presence unlock is unsupported on this platform.':
      '此平台暂不支持自动靠近解锁。',
  'The native phone-key service is not connected yet.': '暂时无法连接手机钥匙服务，请稍后重试。',
  'Refreshing authoritative phone-key state…': '正在刷新手机钥匙状态…',
  'A phone-key update is in progress…': '正在更新手机钥匙，请稍候…',
  'Android connection saved. Next, scan the one-time pairing QR code shown on your Mac.':
      'Android 系统连接已保存。下一步请扫描 Mac 上显示的一次性配对二维码。',
  'Android companion setup did not complete.': 'Android 系统连接尚未完成，请重试。',
  'Phone key revoked.': '手机钥匙已移除。',
  'Native phone-key state is unavailable.': '暂时无法读取手机钥匙状态，请重试。',
  'Native phone-key state could not be refreshed.': '状态刷新失败，请稍后重试。',
  'Device revocation was not confirmed.': '暂未确认移除成功，请刷新状态后再试。',
  'Confirm the device name before pairing.': '请核对设备名称，确认是你自己的设备。',
  'Scan a valid pairing QR code first.': '请先扫描有效的 Repose 配对二维码。',
  'This pairing QR code has expired. Scan a new QR code.':
      '配对二维码已过期，请在 Mac 上生成并扫描新的二维码。',
  'This pairing QR code was already used. Scan a new QR code.':
      '配对二维码已使用，请在 Mac 上生成并扫描新的二维码。',
  'Pairing could not be started.': '配对暂时无法开始，请重试。',
  'Pairing confirmation could not refresh authoritative state.':
      '还无法确认配对结果，请刷新状态后重试。',
  'Pairing connection ended. Scan a new QR code.': '配对连接已结束，请扫描新的二维码。',
  'Pairing confirmation failed.': '配对确认失败，请重试。',
  'Pairing is unavailable until native capability recovers.':
      '手机钥匙恢复可用后，才能继续配对。',
  'Starting calibration…': '正在准备距离校准…',
  'Hold the phone nearby for the near sample.': '请把手机放在平时使用电脑的位置，保持稳定。',
  'Move away and collect the far sample.': '带着手机走远一些，在离开电脑的位置完成采集。',
  'Distance calibration complete.': '距离校准已完成。',
  'Near and far samples overlap. Retry calibration.': '两个位置的信号太接近，请拉开距离后重新校准。',
  'Calibration is unavailable.': '距离校准暂不可用。',
  'Calibration needs attention.': '校准未完成，请重新尝试。',
  'Calibration could not be started.': '无法开始校准，请稍后重试。',
  'Calibration step failed.': '本次采集失败，请重试。',
  'Calibration is unavailable until capability and pairing recover.':
      '设备连接恢复并完成配对后，才能继续校准。',
  'Calibration step was rejected.': '本次采集未通过，请重试。',
  'See setup guidance': '查看使用与权限指南',
  'Check again': '重新检查',
  'Make yourself at home.': '让 Repose，更懂你的手机。',
  'Only when you need it.': '需要时，再授权。',
  'Permissions are explained before any system prompt. You can decide later in Settings.':
      '每项权限都会先说明用途，再由你决定是否允许。之后也可以随时在系统设置中调整。',
  'Nearby devices': '附近设备',
  'Notifications': '通知提醒',
  'Background activity': '后台运行',
  'Not requested in this version': '此版本暂不申请',
  'Allowed': '已允许',
  'Permission needed': '需要你的授权',
  'Allow in Settings': '请在设置中允许',
  'Unable to check': '暂时无法检查',
  'Allow': '去授权',
  'Open settings': '打开设置',
  'Bluetooth access is used to communicate with your paired Mac. This version does not request it yet.':
      '蓝牙权限用于与已配对的 Mac 通信。当前版本尚未开放此功能，暂不申请。',
  'Allow Bluetooth access to communicate with your paired Mac.':
      '允许蓝牙访问后，才能与已配对的 Mac 通信。',
  'Notifications will explain connection issues when supported. This version does not send them.':
      '连接异常提醒开放后，会向你说明通知用途。此版本暂不发送通知。',
  'Optional reminders about phone-key connection issues.':
      '用于接收手机钥匙连接异常提醒，可自行选择是否开启。',
  'Battery optimization is on': '系统电池优化已开启',
  'Background activity is restricted': '后台活动受到限制',
  'No Android battery restriction detected': '未检测到 Android 电池限制',
  'Battery optimization may delay background work. It does not mean the app is blocked.':
      '电池优化可能延迟后台活动，并不代表应用一定无法运行。',
  'Android may stop this app in the background. Review its battery settings if you use phone key.':
      'Android 可能会停止应用的后台活动。使用手机钥匙时，可检查本应用的电池设置。',
  'Your phone may still apply its own background rules. This is not a guarantee of continuous operation.':
      '手机厂商仍可能应用自己的后台策略，此状态不代表应用能始终运行。',
  'Review background settings': '查看后台设置',
  'On realme / OPPO, look for App battery management, Allow background activity, or Auto launch. Names vary by system version.':
      'realme / OPPO 可在应用电池管理中查找「允许后台活动」或「自启动」。具体名称和位置可能随系统版本变化。',
  'Power saving is on': '省电模式已开启',
  'Power saving can delay background activity. You can review it in system settings.':
      '省电模式可能延迟后台响应，需要时可在系统设置中检查。',
  'iOS manages background activity. Automatic presence unlock is not available in this version; changing Background App Refresh will not enable it.':
      'iOS 会管理后台活动。此版本暂不支持自动靠近解锁，开启「后台 App 刷新」也不能启用此功能。',
  'Before opening Settings': '打开设置前，先了解一下',
  'Allowing background activity may use more battery. You can change this later.':
      '允许后台活动可能增加耗电，你可以随时在系统设置中改回来。',
  'This version cannot unlock your Mac automatically, even with background activity allowed.':
      '当前版本即使允许后台活动，也还不能自动解锁 Mac。',
  'Open app settings': '前往应用设置',
  'Not now': '暂时不用',
  'Allow nearby devices?': '允许访问附近设备？',
  'Repose uses Bluetooth to communicate with your paired Mac. You can decline and keep using your Mac password.':
      'Repose 通过蓝牙与已配对的 Mac 通信。你可以暂不授权，继续使用 Mac 密码登录。',
  'Allow notifications?': '允许发送通知？',
  'Receive reminders if your phone key needs attention. This is optional.':
      '手机钥匙需要处理时收到提醒。这是一项可选权限。',
  'Continue': '继续',
  'Could not check settings': '暂时无法检查设置',
  'Try again': '重试',
  'Check your settings again, or use your Mac password for now.':
      '可以重新检查设置，或暂时使用 Mac 密码登录。',
  'Open Settings → Apps → Repose → Permissions or Battery manually.':
      '请手动打开「设置 → 应用 → Repose → 权限或电池」进行检查。',
  'Open Settings → Repose manually.': '请手动打开「设置 → Repose」进行检查。',
  'Your key, your choice.': '你的钥匙，由你掌握。',
  'Pair only your own devices. You can remove a key at any time. Your Mac password remains available.':
      '只配对你自己的设备，不要分享一次性配对二维码。你可以随时移除钥匙，Mac 密码登录始终可用。',
  'Give your day a little space.': '给日常，留一点空白。',
};
