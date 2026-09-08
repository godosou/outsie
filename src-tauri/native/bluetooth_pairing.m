#import <CoreBluetooth/CoreBluetooth.h>
#import <Foundation/Foundation.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

static NSString *const ReposeServiceUUID = @"A53E0001-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeControlUUID = @"A53E0002-7A6B-4D59-9F2E-5245504F5345";
static NSString *const ReposeStatusUUID = @"A53E0003-7A6B-4D59-9F2E-5245504F5345";

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
      if (self.startRequested) [self publishAndAdvertise];
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
}

- (void)publishAndAdvertise {
  if (self.manager.state != CBManagerStatePoweredOn || self.requestedSession.length == 0) return;
  self.currentPublishedService = nil;
  [self.manager stopAdvertising];
  [self.manager removeAllServices];

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
  service.characteristics = @[control, self.statusCharacteristic];
  self.currentPublishedService = service;
  [self.manager addService:service];
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral
             didAddService:(CBService *)service
                     error:(NSError *)error {
  if (service != self.currentPublishedService) return;
  if (error || !self.startRequested || self.requestedSession.length == 0) {
    if (error) {
      self.currentPublishedService = nil;
      self.radioState = ReposeBluetoothRadioFailed;
    }
    return;
  }
  [peripheral startAdvertising:@{
    CBAdvertisementDataLocalNameKey: @"Repose Mac",
    CBAdvertisementDataServiceUUIDsKey: @[[CBUUID UUIDWithString:ReposeServiceUUID]],
  }];
}

- (void)peripheralManager:(CBPeripheralManager *)peripheral
    didReceiveReadRequest:(CBATTRequest *)request {
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
      ![request.characteristic.UUID isEqual:[CBUUID UUIDWithString:ReposeControlUUID]] ||
      request.value.length == 0 || request.value.length > 512) {
    result = request.offset != 0
      ? CBATTErrorInvalidOffset
      : CBATTErrorInvalidAttributeValueLength;
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
      self.statusCharacteristic.value = accepted;
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
  self.currentPublishedService = nil;
  [self.manager stopAdvertising];
  [self.manager removeAllServices];
  self.requestedSession = nil;
  self.peerSession = nil;
  self.peerDeviceIdentifier = nil;
  self.peerDisplayName = nil;
  self.statusCharacteristic = nil;
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
