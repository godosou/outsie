#import <CoreBluetooth/CoreBluetooth.h>
#import <Foundation/Foundation.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <stdlib.h>

static NSString *const ReposeServiceUUID = @"A53E0001-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeControlUUID = @"A53E0002-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeStatusUUID = @"A53E0003-7A6B-4D59-9F2E-5245504F5345";

static NSString *const ReposeConsoleRXUUID = @"A53E0004-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeConsoleTXUUID = @"A53E0005-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeConsoleChallengeUUID = @"A53E0006-7A6B-4D59-9F2E-5245504F5345";
static const NSUInteger ReposeConsoleMaxPacket = 256 * 1024;
static const NSUInteger ReposeConsoleMaxFragments = 24000;
typedef void (*ReposeConsoleCallback)(const char *, int, const uint8_t *, size_t);

typedef NS_ENUM(int32_t, ReposeBluetoothRadioState) {
  ReposeBluetoothRadioUnknown = 0,
  ReposeBluetoothRadioReady = 1,
  ReposeBluetoothRadioPoweredOff = 2,
  ReposeBluetoothRadioUnauthorized = 3,
  ReposeBluetoothRadioUnsupported = 4,
  ReposeBluetoothRadioFailed = 5,
};

@interface ReposeBluetoothPairingPeripheral : NSObject <CBPeripheralManagerDelegate>
@property(nonatomic, strong) CBPeripheralManager *manager;
@property(nonatomic, strong) CBMutableService *currentPublishedService;
@property(nonatomic, strong) CBMutableCharacteristic *statusCharacteristic;
@property(nonatomic, copy) NSString *requestedSession;
@property(nonatomic, copy) NSString *peerSession;
@property(nonatomic, copy) NSString *peerDeviceIdentifier;
@property(nonatomic, copy) NSString *peerDisplayName;
@property(nonatomic) ReposeBluetoothRadioState radioState;
@property(nonatomic) BOOL startRequested;
@property(nonatomic) BOOL serviceReady;
@property(nonatomic) BOOL consoleEnabled;
@property(nonatomic) ReposeConsoleCallback consoleCallback;
@property(nonatomic, strong) CBMutableCharacteristic *consoleTX;
@property(nonatomic, strong) CBCentral *consoleCentral;
@property(nonatomic, strong) NSData *consoleChallenge;
@property(nonatomic, strong) NSMutableData *consoleRX;
@property(nonatomic) uint32_t consoleMessageID;
@property(nonatomic) uint16_t consoleRXCount;
@property(nonatomic) uint16_t consoleRXNext;
@property(nonatomic) BOOL consoleWaitingResponse;
@property(nonatomic, strong) NSData *consoleResponse;
@property(nonatomic) NSUInteger consoleTXNext;
@property(nonatomic) NSUInteger consoleTXCount;
@property(nonatomic) NSUInteger consoleTXPayloadSize;
@property(nonatomic, strong) NSTimer *consoleTimeoutTimer;
@property(nonatomic) NSTimeInterval consoleDeadline;
- (void)publishAndAdvertise;
- (void)clearConsoleConnection;
- (BOOL)startConsole:(ReposeConsoleCallback)callback;
- (void)stopConsole;
- (BOOL)sendConsole:(NSString *)central data:(NSData *)data;
- (BOOL)revokeConsole:(NSString *)central challenge:(NSData *)challenge;
- (void)drainConsoleNotifications;
- (void)checkConsoleTimeout;
- (void)notifyConsoleRadio;
- (CBATTError)receiveConsoleFragment:(NSData *)value central:(CBCentral *)central;
@end

static BOOL ReposeValidIdentifier(NSString *value) {
  if (value.length == 0 || value.length > 64) return NO;
  NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:
    @"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_"];
  return [value rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound;
}

static NSString *ReposeSafeDisplayName(NSString *value) {
  if (value.length == 0 || value.length > 80) return nil;
  if ([value rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound) {
    return nil;
  }
  return value;
}

@implementation ReposeBluetoothPairingPeripheral

- (instancetype)init {
  self = [super init];
  if (self) {
    _radioState = ReposeBluetoothRadioUnknown;
    _manager = [[CBPeripheralManager alloc] initWithDelegate:self
                                                       queue:dispatch_get_main_queue()
                                                     options:@{CBPeripheralManagerOptionShowPowerAlertKey: @YES}];
  }
  return self;
}

- (void)peripheralManagerDidUpdateState:(CBPeripheralManager *)peripheral {
  switch (peripheral.state) {
    case CBManagerStatePoweredOn:
      self.radioState = ReposeBluetoothRadioReady;
      if (self.startRequested || self.consoleEnabled) [self publishAndAdvertise];
      break;
    case CBManagerStatePoweredOff:
      self.radioState = ReposeBluetoothRadioPoweredOff;
      break;
    case CBManagerStateUnauthorized:
      self.radioState = ReposeBluetoothRadioUnauthorized;
      break;
    case CBManagerStateUnsupported:
      self.radioState = ReposeBluetoothRadioUnsupported;
      break;
    case CBManagerStateResetting:
    case CBManagerStateUnknown:
      self.radioState = ReposeBluetoothRadioUnknown;
      break;
  }
  if (peripheral.state != CBManagerStatePoweredOn) {
    [self clearConsoleConnection];
    self.currentPublishedService = nil;
    self.serviceReady = NO;
  }
  [self notifyConsoleRadio];
}

- (void)publishAndAdvertise {
  if (self.manager.state != CBManagerStatePoweredOn || (!self.startRequested && !self.consoleEnabled)) return;
  // Both features own the same publication; changing a pairing session must not
  // invalidate an already connected console's characteristics.
  if (self.currentPublishedService) {
    if (self.serviceReady && !self.manager.isAdvertising) {
      [self.manager startAdvertising:@{
        CBAdvertisementDataLocalNameKey: @"Repose Mac",
        CBAdvertisementDataServiceUUIDsKey: @[[CBUUID UUIDWithString:ReposeServiceUUID]],
      }];
    }
    return;
  }
  [self.manager stopAdvertising];
  [self.manager removeAllServices];
  self.serviceReady = NO;

  CBMutableCharacteristic *control = [[CBMutableCharacteristic alloc]
    initWithType:[CBUUID UUIDWithString:ReposeControlUUID]
      properties:CBCharacteristicPropertyWrite
           value:nil
     permissions:CBAttributePermissionsWriteable];
  self.statusCharacteristic = [[CBMutableCharacteristic alloc]
    initWithType:[CBUUID UUIDWithString:ReposeStatusUUID]
      properties:(CBCharacteristicPropertyRead | CBCharacteristicPropertyNotify)
           value:nil
     permissions:CBAttributePermissionsReadable];
  CBMutableService *service = [[CBMutableService alloc]
    initWithType:[CBUUID UUIDWithString:ReposeServiceUUID]
         primary:YES];
  CBMutableCharacteristic *consoleRX = [[CBMutableCharacteristic alloc]
    initWithType:[CBUUID UUIDWithString:ReposeConsoleRXUUID]
      properties:CBCharacteristicPropertyWrite value:nil permissions:CBAttributePermissionsWriteable];
  self.consoleTX = [[CBMutableCharacteristic alloc]
    initWithType:[CBUUID UUIDWithString:ReposeConsoleTXUUID]
      properties:CBCharacteristicPropertyNotify value:nil permissions:CBAttributePermissionsReadable];
  CBMutableCharacteristic *challenge = [[CBMutableCharacteristic alloc]
    initWithType:[CBUUID UUIDWithString:ReposeConsoleChallengeUUID]
      properties:CBCharacteristicPropertyRead value:nil permissions:CBAttributePermissionsReadable];
  service.characteristics = @[control, self.statusCharacteristic, consoleRX, self.consoleTX, challenge];
  self.currentPublishedService = service;
  [self.manager addService:service];
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral
             didAddService:(CBService *)service
                     error:(NSError *)error {
  if (service != self.currentPublishedService) return;
  if (error || (!self.startRequested && !self.consoleEnabled)) {
    if (error) {
      self.currentPublishedService = nil;
      self.radioState = ReposeBluetoothRadioFailed;
      [self notifyConsoleRadio];
    }
    return;
  }
  self.serviceReady = YES;
  [peripheral startAdvertising:@{
    CBAdvertisementDataLocalNameKey: @"Repose Mac",
    CBAdvertisementDataServiceUUIDsKey: @[[CBUUID UUIDWithString:ReposeServiceUUID]],
  }];
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral
    didReceiveReadRequest:(CBATTRequest *)request {
  if ([request.characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeConsoleChallengeUUID]]) {
    if (request.offset != 0) {
      [peripheral respondToRequest:request withResult:CBATTErrorInvalidOffset];
    } else if (!self.consoleEnabled || !self.consoleChallenge ||
               ![request.central.identifier isEqual:self.consoleCentral.identifier]) {
      [peripheral respondToRequest:request withResult:CBATTErrorReadNotPermitted];
    } else {
      request.value = self.consoleChallenge;
      [peripheral respondToRequest:request withResult:CBATTErrorSuccess];
    }
    return;
  }
  if (![request.characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeStatusUUID]]) {
    [peripheral respondToRequest:request withResult:CBATTErrorAttributeNotFound];
    return;
  }
  if (request.offset != 0) {
    [peripheral respondToRequest:request withResult:CBATTErrorInvalidOffset];
    return;
  }
  NSString *status = self.peerSession.length > 0
    ? [NSString stringWithFormat:@"ACCEPTED|%@", self.peerSession]
    : [NSString stringWithFormat:@"WAITING|%@", self.requestedSession ?: @""];
  request.value = [status dataUsingEncoding:NSUTF8StringEncoding];
  [peripheral respondToRequest:request withResult:CBATTErrorSuccess];
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral
    didReceiveWriteRequests:(NSArray<CBATTRequest *> *)requests {
  CBATTRequest *request = requests.firstObject;
  if (!request) return;

  CBATTError result = CBATTErrorSuccess;
  if (requests.count != 1 || request.offset != 0 ||
      request.value.length == 0 || request.value.length > 512) {
    result = request.offset != 0
      ? CBATTErrorInvalidOffset
      : CBATTErrorInvalidAttributeValueLength;
  } else if ([request.characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeConsoleRXUUID]]) {
    result = [self receiveConsoleFragment:request.value central:request.central];
  } else if (![request.characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeControlUUID]] ||
             !self.startRequested) {
    result = CBATTErrorWriteNotPermitted;
  } else {
    NSString *frame = [[NSString alloc] initWithData:request.value encoding:NSUTF8StringEncoding];
    NSArray<NSString *> *parts = [frame componentsSeparatedByString:@"|"];
    if (parts.count != 4 || ![parts[0] isEqualToString:@"RPD1"] ||
        ![parts[1] isEqualToString:self.requestedSession] ||
        !ReposeValidIdentifier(parts[1]) || !ReposeValidIdentifier(parts[2]) ||
        ReposeSafeDisplayName(parts[3]) == nil) {
      result = CBATTErrorWriteNotPermitted;
    } else {
      self.peerSession = parts[1];
      self.peerDeviceIdentifier = parts[2];
      self.peerDisplayName = parts[3];
      NSData *accepted = [[NSString stringWithFormat:@"ACCEPTED|%@", self.peerSession]
        dataUsingEncoding:NSUTF8StringEncoding];
      // Keep the characteristic dynamic across pairing sessions; reads are
      // answered above from the current peer/session rather than cached bytes.
      [peripheral updateValue:accepted forCharacteristic:self.statusCharacteristic onSubscribedCentrals:nil];
    }
  }
  [peripheral respondToRequest:request withResult:result];
}

- (BOOL)startSession:(NSString *)session {
  if (!ReposeValidIdentifier(session)) return NO;
  self.requestedSession = session;
  self.peerSession = nil;
  self.peerDeviceIdentifier = nil;
  self.peerDisplayName = nil;
  self.startRequested = YES;
  if (self.manager.state == CBManagerStatePoweredOn) [self publishAndAdvertise];
  return YES;
}

- (void)stopSession {
  self.startRequested = NO;
  if (!self.consoleEnabled) {
    self.currentPublishedService = nil;
    self.serviceReady = NO;
    [self.manager stopAdvertising];
    [self.manager removeAllServices];
    self.statusCharacteristic = nil;
    self.consoleTX = nil;
  }
  self.requestedSession = nil;
  self.peerSession = nil;
  self.peerDeviceIdentifier = nil;
  self.peerDisplayName = nil;
}

// All console state belongs to the main queue. Callbacks must only copy/enqueue
// on the Rust side; they cannot synchronously call back while holding Rust locks.
- (void)clearConsoleConnection {
  NSString *identifier = self.consoleCentral.identifier.UUIDString;
  [self.consoleTimeoutTimer invalidate];
  self.consoleTimeoutTimer = nil;
  self.consoleDeadline = 0;
  self.consoleCentral = nil;
  self.consoleChallenge = nil;
  self.consoleRX = nil;
  self.consoleResponse = nil;
  self.consoleWaitingResponse = NO;
  self.consoleRXNext = self.consoleRXCount = 0;
  self.consoleTXNext = self.consoleTXCount = 0;
  if (identifier && self.consoleCallback) self.consoleCallback(identifier.UTF8String, 3, NULL, 0);
}

- (BOOL)startConsole:(ReposeConsoleCallback)callback {
  if (!callback) return NO;
  [self clearConsoleConnection];
  self.consoleCallback = callback;
  self.consoleEnabled = YES;
  [self notifyConsoleRadio];
  [self publishAndAdvertise];
  return YES;
}

- (void)notifyConsoleRadio {
  uint8_t state = (uint8_t)self.radioState;
  if (self.consoleCallback) self.consoleCallback("", 4, &state, 1);
}

- (void)stopConsole {
  self.consoleEnabled = NO;
  [self clearConsoleConnection];
  self.consoleCallback = NULL;
  if (!self.startRequested) {
    self.currentPublishedService = nil;
    self.serviceReady = NO;
    [self.manager stopAdvertising];
    [self.manager removeAllServices];
    self.statusCharacteristic = nil;
    self.consoleTX = nil;
  }
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral central:(CBCentral *)central
    didSubscribeToCharacteristic:(CBCharacteristic *)characteristic {
  if (![characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeConsoleTXUUID]] || !self.consoleEnabled) return;
  if (self.consoleCentral && ![self.consoleCentral.identifier isEqual:central.identifier]) return;
  [self clearConsoleConnection];
  uint8_t challenge[16];
  arc4random_buf(challenge, sizeof(challenge));
  self.consoleCentral = central;
  self.consoleChallenge = [NSData dataWithBytes:challenge length:sizeof(challenge)];
  self.consoleDeadline = NSProcessInfo.processInfo.systemUptime + 30;
  __weak ReposeBluetoothPairingPeripheral *weakSelf = self;
  self.consoleTimeoutTimer = [NSTimer scheduledTimerWithTimeInterval:1 repeats:YES block:^(NSTimer * __unused timer) {
    [weakSelf checkConsoleTimeout];
  }];
  if (self.consoleCallback) self.consoleCallback(central.identifier.UUIDString.UTF8String, 1, challenge, sizeof(challenge));
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral central:(CBCentral *)central
    didUnsubscribeFromCharacteristic:(CBCharacteristic *)characteristic {
  if ([characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeConsoleTXUUID]] &&
      [self.consoleCentral.identifier isEqual:central.identifier]) [self clearConsoleConnection];
}

- (CBATTError)receiveConsoleFragment:(NSData *)value central:(CBCentral *)central {
  if (!self.consoleEnabled || !self.consoleChallenge ||
      ![self.consoleCentral.identifier isEqual:central.identifier]) return CBATTErrorWriteNotPermitted;
  // An inbound message cannot replace the ID used by the pending response.
  if (self.consoleWaitingResponse) return CBATTErrorWriteNotPermitted;
  if (value.length <= 9 || value.length > 512) {
    [self clearConsoleConnection];
    return CBATTErrorInvalidAttributeValueLength;
  }
  const uint8_t *bytes = value.bytes;
  uint32_t messageID = ((uint32_t)bytes[1] << 24) | ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 8) | bytes[4];
  uint16_t index = ((uint16_t)bytes[5] << 8) | bytes[6];
  uint16_t count = ((uint16_t)bytes[7] << 8) | bytes[8];
  if (bytes[0] != 0xC1 || count == 0 || count > ReposeConsoleMaxFragments || index >= count) {
    [self clearConsoleConnection];
    return CBATTErrorInvalidAttributeValueLength;
  }
  if (index == 0) {
    BOOL alreadyReceiving = self.consoleRX != nil;
    self.consoleRX = [NSMutableData data];
    self.consoleMessageID = messageID;
    self.consoleRXCount = count;
    self.consoleRXNext = 0;
    // Restarting fragment zero does not extend the original deadline. One
    // timer per connection bounds resources even if a peer floods requests.
    if (!alreadyReceiving && self.consoleDeadline == 0) self.consoleDeadline = NSProcessInfo.processInfo.systemUptime + 30;
  }
  if (!self.consoleRX || messageID != self.consoleMessageID || count != self.consoleRXCount ||
      index != self.consoleRXNext || self.consoleRX.length + value.length - 9 > ReposeConsoleMaxPacket) {
    [self clearConsoleConnection];
    return CBATTErrorInvalidAttributeValueLength;
  }
  [self.consoleRX appendBytes:bytes + 9 length:value.length - 9];
  self.consoleRXNext++;
  if (self.consoleRXNext == count) {
    NSData *packet = self.consoleRX;
    self.consoleRX = nil;
    self.consoleWaitingResponse = YES;
    if (self.consoleCallback) self.consoleCallback(central.identifier.UUIDString.UTF8String, 2, packet.bytes, packet.length);
  }
  return CBATTErrorSuccess;
}

- (BOOL)sendConsole:(NSString *)central data:(NSData *)data {
  if (!self.consoleEnabled || !self.consoleCentral ||
      ![self.consoleCentral.identifier.UUIDString isEqualToString:central]) return NO;
  // Revocation requires a challenge as well; never allow an old worker's empty
  // response to clear a newer subscription with the same central identifier.
  if (data.length == 0) return NO;
  if (!self.consoleWaitingResponse || self.consoleResponse || data.length > ReposeConsoleMaxPacket) return NO;
  // A worker from an old subscription must not inject a response into a new
  // request, even when CoreBluetooth reuses the same central identifier.
  if (data.length < 58 || self.consoleChallenge.length != 16 ||
      memcmp((const uint8_t *)data.bytes + 18, self.consoleChallenge.bytes, 16) != 0) return NO;
  NSUInteger mtu = MIN(self.consoleCentral.maximumUpdateValueLength, 512);
  if (mtu <= 9) { [self clearConsoleConnection]; return NO; }
  self.consoleTXPayloadSize = mtu - 9;
  self.consoleTXCount = (data.length + self.consoleTXPayloadSize - 1) / self.consoleTXPayloadSize;
  if (self.consoleTXCount > ReposeConsoleMaxFragments) { [self clearConsoleConnection]; return NO; }
  self.consoleTXNext = 0;
  self.consoleResponse = data;
  [self drainConsoleNotifications];
  return YES;
}

- (BOOL)revokeConsole:(NSString *)central challenge:(NSData *)challenge {
  if (!self.consoleEnabled || !self.consoleCentral || challenge.length != 16 ||
      ![self.consoleCentral.identifier.UUIDString isEqualToString:central] ||
      ![self.consoleChallenge isEqualToData:challenge]) return NO;
  // CoreBluetooth peripheral cannot force a physical disconnect. Reject further
  // reads/writes until the central resubscribes and obtains a fresh challenge.
  [self clearConsoleConnection];
  return YES;
}

- (void)drainConsoleNotifications {
  while (self.consoleResponse && self.consoleCentral && self.consoleTXNext < self.consoleTXCount) {
    NSUInteger index = self.consoleTXNext;
    uint32_t messageID = self.consoleMessageID;
    uint8_t header[9] = {0xC1, messageID >> 24, messageID >> 16, messageID >> 8, messageID,
      index >> 8, index, self.consoleTXCount >> 8, self.consoleTXCount};
    NSMutableData *fragment = [NSMutableData dataWithBytes:header length:sizeof(header)];
    NSUInteger offset = index * self.consoleTXPayloadSize;
    NSUInteger length = MIN(self.consoleTXPayloadSize, self.consoleResponse.length - offset);
    [fragment appendBytes:(const uint8_t *)self.consoleResponse.bytes + offset length:length];
    if (![self.manager updateValue:fragment forCharacteristic:self.consoleTX onSubscribedCentrals:@[self.consoleCentral]]) return;
    self.consoleTXNext++;
  }
  if (self.consoleResponse && self.consoleTXNext == self.consoleTXCount) {
    self.consoleResponse = nil;
    self.consoleWaitingResponse = NO;
    self.consoleDeadline = 0;
  }
}

- (void)checkConsoleTimeout {
  if (self.consoleDeadline > 0 && NSProcessInfo.processInfo.systemUptime >= self.consoleDeadline) {
    [self clearConsoleConnection];
  }
}

- (void)peripheralManagerIsReadyToUpdateSubscribers:(CBPeripheralManager *)peripheral {
  [self drainConsoleNotifications];
}

@end

static ReposeBluetoothPairingPeripheral *ReposePeripheral(void) {
  static ReposeBluetoothPairingPeripheral *peripheral;
  static dispatch_once_t onceToken;
  dispatch_once(&onceToken, ^{ peripheral = [[ReposeBluetoothPairingPeripheral alloc] init]; });
  return peripheral;
}

static void ReposeOnMainSync(dispatch_block_t block) {
  if (NSThread.isMainThread) block();
  else dispatch_sync(dispatch_get_main_queue(), block);
}

int32_t repose_ble_pairing_radio_state(void) {
  __block int32_t result = ReposeBluetoothRadioUnknown;
  ReposeOnMainSync(^{ result = (int32_t)ReposePeripheral().radioState; });
  return result;
}

bool repose_ble_pairing_start(const char *raw_session) {
  if (!raw_session) return false;
  NSString *session = [NSString stringWithUTF8String:raw_session];
  if (!session) return false;
  __block BOOL result = NO;
  ReposeOnMainSync(^{ result = [ReposePeripheral() startSession:session]; });
  return result;
}

void repose_ble_pairing_stop(void) {
  ReposeOnMainSync(^{ [ReposePeripheral() stopSession]; });
}

bool repose_ble_pairing_has_peer(void) {
  __block BOOL result = NO;
  ReposeOnMainSync(^{ result = ReposePeripheral().peerSession.length > 0; });
  return result;
}

static size_t ReposeCopyString(NSString *value, char *buffer, size_t capacity) {
  if (!value) return 0;
  NSData *data = [value dataUsingEncoding:NSUTF8StringEncoding];
  size_t required = data.length;
  if (!buffer || capacity == 0) return required;
  size_t copied = MIN(required, capacity - 1);
  memcpy(buffer, data.bytes, copied);
  buffer[copied] = '\0';
  return required;
}

size_t repose_ble_pairing_copy_peer_session(char *buffer, size_t capacity) {
  __block size_t result = 0;
  ReposeOnMainSync(^{ result = ReposeCopyString(ReposePeripheral().peerSession, buffer, capacity); });
  return result;
}

size_t repose_ble_pairing_copy_peer_identifier(char *buffer, size_t capacity) {
  __block size_t result = 0;
  ReposeOnMainSync(^{ result = ReposeCopyString(ReposePeripheral().peerDeviceIdentifier, buffer, capacity); });
  return result;
}

size_t repose_ble_pairing_copy_peer_name(char *buffer, size_t capacity) {
  __block size_t result = 0;
  ReposeOnMainSync(^{ result = ReposeCopyString(ReposePeripheral().peerDisplayName, buffer, capacity); });
  return result;
}


bool repose_ble_console_start(ReposeConsoleCallback callback) {
  if (!callback) return false;
  __block BOOL result = NO;
  ReposeOnMainSync(^{ result = [ReposePeripheral() startConsole:callback]; });
  return result;
}

void repose_ble_console_stop(void) {
  ReposeOnMainSync(^{ [ReposePeripheral() stopConsole]; });
}

bool repose_ble_console_send(const char *raw_central, const uint8_t *bytes, size_t len) {
  if (!raw_central || !bytes || len == 0 || len > ReposeConsoleMaxPacket) return false;
  NSString *central = [NSString stringWithUTF8String:raw_central];
  if (!central) return false;
  NSData *data = [NSData dataWithBytes:bytes length:len];
  __block BOOL result = NO;
  ReposeOnMainSync(^{ result = [ReposePeripheral() sendConsole:central data:data]; });
  return result;
}

bool repose_ble_console_revoke(const char *raw_central, const uint8_t *expected_challenge, size_t len) {
  if (!raw_central || !expected_challenge || len != 16) return false;
  NSString *central = [NSString stringWithUTF8String:raw_central];
  if (!central) return false;
  NSData *challenge = [NSData dataWithBytes:expected_challenge length:len];
  __block BOOL result = NO;
  ReposeOnMainSync(^{ result = [ReposePeripheral() revokeConsole:central challenge:challenge]; });
  return result;
}
