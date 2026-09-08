// Hardware-free native simulation. Compile and run on macOS:
// clang -fobjc-arc tests/native_bluetooth_console.m -framework Foundation \
//   -framework CoreBluetooth -o /tmp/repose-native-console-test && /tmp/repose-native-console-test
#import "../native/bluetooth_pairing.m"

static NSMutableArray<NSDictionary *> *events;
static void capture(const char *central, int event, const uint8_t *bytes, size_t length) {
  [events addObject:@{@"central":@(central), @"event":@(event), @"data":[NSData dataWithBytes:bytes length:length]}];
}
@interface FakeConsoleCentral : NSObject
@property(nonatomic, strong) NSUUID *identifier;
@property(nonatomic) NSUInteger maximumUpdateValueLength;
@end
@implementation FakeConsoleCentral @end
@interface FakeConsoleManager : NSObject
@property(nonatomic) CBManagerState state;
@property(nonatomic) BOOL isAdvertising;
@property(nonatomic) NSUInteger removeCount;
@property(nonatomic) NSUInteger addCount;
@property(nonatomic) NSUInteger acceptBudget;
@property(nonatomic, strong) NSMutableArray<NSData *> *sent;
- (void)stopAdvertising;
- (void)removeAllServices;
- (void)addService:(CBMutableService *)service;
- (void)startAdvertising:(NSDictionary *)data;
- (BOOL)updateValue:(NSData *)data forCharacteristic:(CBMutableCharacteristic *)characteristic onSubscribedCentrals:(NSArray *)centrals;
@end
@implementation FakeConsoleManager
- (void)stopAdvertising { self.isAdvertising = NO; }
- (void)removeAllServices { self.removeCount++; }
- (void)addService:(CBMutableService *)service { self.addCount++; }
- (void)startAdvertising:(NSDictionary *)data { self.isAdvertising = YES; }
- (BOOL)updateValue:(NSData *)data forCharacteristic:(CBMutableCharacteristic *)characteristic onSubscribedCentrals:(NSArray *)centrals {
  if (self.acceptBudget == 0) return NO;
  NSCAssert(centrals.count == 1, @"Never notify unrelated subscribers");
  self.acceptBudget--;
  [self.sent addObject:data];
  return YES;
}
@end
static NSData *fragment(uint32_t messageID, uint16_t index, uint16_t count, NSData *payload) {
  uint8_t header[] = {0xC1,messageID>>24,messageID>>16,messageID>>8,messageID,index>>8,index,count>>8,count};
  NSMutableData *value = [NSMutableData dataWithBytes:header length:9];
  [value appendData:payload];
  return value;
}
static void subscribe(ReposeBluetoothPairingPeripheral *peripheral, FakeConsoleCentral *central) {
  [peripheral peripheralManager:peripheral.manager central:(id)central didSubscribeToCharacteristic:peripheral.consoleTX];
}
static NSData *response(NSData *challenge) {
  NSMutableData *packet = [NSMutableData dataWithLength:80];
  uint8_t *bytes = packet.mutableBytes;
  bytes[0] = 1; bytes[1] = 1;
  memcpy(bytes+18, challenge.bytes, 16);
  return packet;
}
int main(void) {
  @autoreleasepool {
    events = [NSMutableArray array];
    FakeConsoleManager *manager = [FakeConsoleManager new];
    manager.state = CBManagerStatePoweredOn;
    manager.sent = [NSMutableArray array];
    // Deliberately skip -init: the real initializer opens the Bluetooth radio.
    ReposeBluetoothPairingPeripheral *peripheral = [ReposeBluetoothPairingPeripheral alloc];
    peripheral.manager = (id)manager;
    peripheral.radioState = ReposeBluetoothRadioReady;
    NSCAssert([peripheral startSession:@"session-1"], @"Pairing starts");
    CBMutableService *service = peripheral.currentPublishedService;
    NSCAssert(service.characteristics.count == 5, @"Pairing and console share five characteristics");
    [peripheral peripheralManager:(id)manager didAddService:service error:nil];
    NSCAssert([peripheral startConsole:capture], @"Console starts");
    NSCAssert([events.lastObject[@"event"] intValue] == 4 && ((const uint8_t *)[events.lastObject[@"data"] bytes])[0] == 1, @"Start publishes cached radio readiness");
    [events removeAllObjects];
    NSCAssert(manager.addCount == 1 && manager.removeCount == 1, @"No republish during console start");
    [peripheral stopSession];
    NSCAssert(peripheral.currentPublishedService == service && manager.removeCount == 1, @"Pairing stop preserves console");
    NSCAssert([peripheral startSession:@"session-2"], @"New pairing session starts");
    NSCAssert(peripheral.currentPublishedService == service && manager.addCount == 1, @"Pairing update preserves service");
    FakeConsoleCentral *first = [FakeConsoleCentral new];
    first.identifier = [NSUUID UUID]; first.maximumUpdateValueLength = 20;
    FakeConsoleCentral *other = [FakeConsoleCentral new];
    other.identifier = [NSUUID UUID]; other.maximumUpdateValueLength = 512;
    subscribe(peripheral, first);
    NSData *oldChallenge = peripheral.consoleChallenge;
    NSCAssert(events.count == 1 && [events.lastObject[@"data"] length] == 16, @"Fresh 16-byte challenge");
    subscribe(peripheral, other);
    NSCAssert(events.count == 1 && peripheral.consoleCentral == (id)first, @"Second central cannot steal connection");
    NSData *part = [@"payload" dataUsingEncoding:NSUTF8StringEncoding];
    NSCAssert([peripheral receiveConsoleFragment:fragment(7,0,2,part) central:(id)other] == CBATTErrorWriteNotPermitted, @"Other central cannot write");
    NSCAssert([peripheral receiveConsoleFragment:fragment(7,0,2,part) central:(id)first] == CBATTErrorSuccess, @"First ordered fragment");
    NSCAssert([peripheral receiveConsoleFragment:fragment(8,1,2,part) central:(id)first] != CBATTErrorSuccess, @"Mismatched message rejected");
    NSCAssert(peripheral.consoleCentral == nil && [events.lastObject[@"event"] intValue] == 3, @"Invalid sequence clears authorization");
    subscribe(peripheral, first);
    NSCAssert(![oldChallenge isEqual:peripheral.consoleChallenge], @"Reconnection rotates challenge");
    NSCAssert([peripheral receiveConsoleFragment:fragment(7,0,2,part) central:(id)first] == CBATTErrorSuccess, @"Start message");
    NSCAssert([peripheral receiveConsoleFragment:fragment(7,1,2,part) central:(id)first] == CBATTErrorSuccess, @"Finish message");
    NSCAssert(peripheral.consoleWaitingResponse && [events.lastObject[@"event"] intValue] == 2 && [events.lastObject[@"data"] length] == part.length*2, @"Only assembled packet dispatched");
    NSCAssert([peripheral receiveConsoleFragment:fragment(8,0,1,part) central:(id)first] != CBATTErrorSuccess && peripheral.consoleMessageID == 7, @"Pending response retains message ID");
    NSCAssert(![peripheral sendConsole:first.identifier.UUIDString data:response(oldChallenge)], @"Stale worker response rejected");
    NSData *reply = response(peripheral.consoleChallenge);
    manager.acceptBudget = 1;
    NSCAssert([peripheral sendConsole:first.identifier.UUIDString data:reply], @"Response queued");
    NSCAssert(manager.sent.count == 1 && peripheral.consoleTXNext == 1 && peripheral.consoleWaitingResponse, @"Backpressure keeps pending fragments");
    manager.acceptBudget = 100;
    [peripheral peripheralManagerIsReadyToUpdateSubscribers:(id)manager];
    NSMutableData *assembled = [NSMutableData data];
    for (NSUInteger index=0; index<manager.sent.count; index++) {
      NSData *frame = manager.sent[index]; const uint8_t *bytes = frame.bytes;
      NSCAssert(frame.length <= 20 && bytes[0] == 0xC1 && bytes[4] == 7 && bytes[6] == index, @"MTU, message ID and ordering preserved");
      [assembled appendBytes:bytes+9 length:frame.length-9];
    }
    NSCAssert([assembled isEqual:reply] && !peripheral.consoleWaitingResponse, @"Backpressure resume produces exact response");
    NSCAssert(![peripheral sendConsole:first.identifier.UUIDString data:[NSData data]] && peripheral.consoleCentral != nil, @"Empty sends cannot revoke a subscription");
    NSCAssert(![peripheral revokeConsole:first.identifier.UUIDString challenge:oldChallenge] && peripheral.consoleCentral != nil, @"Old worker cannot revoke a newer challenge");
    NSCAssert(![peripheral revokeConsole:other.identifier.UUIDString challenge:peripheral.consoleChallenge] && peripheral.consoleCentral != nil, @"Revocation is bound to central as well as challenge");
    NSCAssert([peripheral revokeConsole:first.identifier.UUIDString challenge:peripheral.consoleChallenge], @"Matching challenge revokes logical connection");
    NSCAssert(peripheral.consoleCentral == nil && [events.lastObject[@"event"] intValue] == 3, @"Revocation event");
    subscribe(peripheral, first);
    NSData *fullFragment = [NSMutableData dataWithLength:503];
    CBATTError sizeResult = CBATTErrorSuccess;
    for (uint16_t index=0; index<600 && sizeResult == CBATTErrorSuccess; index++) {
      sizeResult = [peripheral receiveConsoleFragment:fragment(8,index,600,fullFragment) central:(id)first];
    }
    NSCAssert(sizeResult != CBATTErrorSuccess && peripheral.consoleCentral == nil, @"Aggregate packet bound enforced");
    subscribe(peripheral, first);
    NSCAssert([peripheral receiveConsoleFragment:fragment(8,0,24001,part) central:(id)first] != CBATTErrorSuccess && peripheral.consoleCentral == nil, @"Fragment count bound enforced");
    subscribe(peripheral, first);
    first.maximumUpdateValueLength = 1000;
    NSCAssert([peripheral receiveConsoleFragment:fragment(8,0,1,part) central:(id)first] == CBATTErrorSuccess, @"Start large-MTU response");
    NSMutableData *largeReply = [NSMutableData dataWithLength:1200];
    memcpy((uint8_t *)largeReply.mutableBytes+18, peripheral.consoleChallenge.bytes, 16);
    [manager.sent removeAllObjects]; manager.acceptBudget = 100;
    NSCAssert([peripheral sendConsole:first.identifier.UUIDString data:largeReply] && manager.sent.count == 3, @"Large MTU still capped at512");
    for (NSData *frame in manager.sent) NSCAssert(frame.length <= 512, @"ATT value upper bound");
    NSCAssert([peripheral receiveConsoleFragment:fragment(9,0,2,part) central:(id)first] == CBATTErrorSuccess, @"Start timeout case");
    NSTimeInterval originalDeadline = peripheral.consoleDeadline;
    NSCAssert([peripheral receiveConsoleFragment:fragment(10,0,2,part) central:(id)first] == CBATTErrorSuccess && peripheral.consoleDeadline == originalDeadline, @"Fragment zero reset does not extend deadline");
    peripheral.consoleDeadline = NSProcessInfo.processInfo.systemUptime - 1;
    [peripheral checkConsoleTimeout];
    NSCAssert(peripheral.consoleCentral == nil && peripheral.consoleTimeoutTimer == nil, @"Timeout revokes connection and timer");
    subscribe(peripheral, first);
    manager.state = CBManagerStatePoweredOff;
    [peripheral peripheralManagerDidUpdateState:(id)manager];
    NSCAssert(peripheral.consoleCentral == nil && peripheral.currentPublishedService == nil, @"Radio off invalidates challenge and publication");
    NSCAssert([events.lastObject[@"event"] intValue] == 4 && ((const uint8_t *)[events.lastObject[@"data"] bytes])[0] == 2, @"Radio off emits new cached readiness");
    [peripheral stopConsole];
    NSCAssert(peripheral.startRequested, @"Console stop preserves pairing ownership");
    [peripheral stopSession];
    NSCAssert(peripheral.currentPublishedService == nil, @"Final owner removes publication");
    puts("PASS: native BLE service lifecycle, central isolation, fragment ordering, correlation, challenge rotation, MTU, backpressure and revocation (no radio)");
  }
}
