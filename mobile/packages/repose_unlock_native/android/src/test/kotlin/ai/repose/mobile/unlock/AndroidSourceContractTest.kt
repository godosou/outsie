package ai.repose.mobile.unlock

import java.nio.file.Files
import java.nio.file.Path
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

class AndroidSourceContractTest {
    private val workspace = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
    private val plugin = workspace.resolve("mobile/packages/repose_unlock_native")
    private val androidNamespace = "http://schemas.android.com/apk/res/android"

    @Test
    fun `manifest exposes exactly one protected primary companion service`() {
        val manifest = plugin.resolve("android/src/main/AndroidManifest.xml")
        val document = DocumentBuilderFactory.newInstance().apply {
            isNamespaceAware = true
        }.newDocumentBuilder().parse(manifest.toFile())
        val services = document.getElementsByTagName("service")
        val companions = (0 until services.length)
            .map { services.item(it) as Element }
            .filter { service ->
                service.getElementsByTagName("action").asElements().any { action ->
                    action.getAttributeNS(androidNamespace, "name") ==
                        "android.companion.CompanionDeviceService"
                }
            }

        assertEquals(1, companions.size)
        val service = companions.single()
        assertEquals("true", service.getAttributeNS(androidNamespace, "exported"))
        assertEquals(
            "android.permission.BIND_COMPANION_DEVICE_SERVICE",
            service.getAttributeNS(androidNamespace, "permission"),
        )
        val primaryProperties = service.getElementsByTagName("property").asElements().filter {
            it.getAttributeNS(androidNamespace, "name") ==
                "android.companion.PROPERTY_PRIMARY_COMPANION_DEVICE_SERVICE" &&
                it.getAttributeNS(androidNamespace, "value") == "true"
        }
        assertEquals(1, primaryProperties.size)

        val requestedPermissions = document.getElementsByTagName("uses-permission").asElements()
            .map { it.getAttributeNS(androidNamespace, "name") }
        assertEquals(
            1,
            requestedPermissions.count {
                it == "android.permission.REQUEST_OBSERVE_COMPANION_DEVICE_PRESENCE"
            },
        )
        assertFalse("BIND permission belongs on the service", requestedPermissions.contains(
            "android.permission.BIND_COMPANION_DEVICE_SERVICE",
        ))
        assertEquals(
            1,
            requestedPermissions.count { it == "android.permission.BLUETOOTH_SCAN" },
        )
        assertEquals(
            1,
            requestedPermissions.count { it == "android.permission.BLUETOOTH_CONNECT" },
        )

        val features = document.getElementsByTagName("uses-feature").asElements()
        assertEquals(
            1,
            features.count {
                it.getAttributeNS(androidNamespace, "name") ==
                    "android.software.companion_device_setup"
            },
        )
    }

    @Test
    fun `android adapter builds association id requests only`() {
        val sourceRoot = plugin.resolve("android/src/main/kotlin")
        val sources = Files.walk(sourceRoot).use { paths ->
            paths.iterator().asSequence()
                .filter { Files.isRegularFile(it) && it.toString().endsWith(".kt") }
                .map { it.toFile().readText() }
                .toList()
        }
        val joined = sources.joinToString("\n")

        assertTrue(joined.contains("manager.myAssociations"))
        assertTrue(joined.contains("ObservingDevicePresenceRequest.Builder()"))
        assertTrue(joined.contains(".setAssociationId(associationId)"))
        assertFalse(joined.contains(".setUuid("))
    }

    @Test
    fun `system presence callback never starts flutter`() {
        val service = (
            plugin.resolve(
                "android/src/main/kotlin/ai/repose/mobile/unlock/companion/" +
                    "ReposeCompanionDeviceService.kt",
            )
        ).toFile().readText()

        assertTrue(service.contains("override fun onDevicePresenceEvent(event: DevicePresenceEvent)"))
        assertTrue(service.contains("ReposeUnlockRuntime.enqueuePresenceEvent"))
        listOf("FlutterEngine", "FlutterInjector", "GeneratedPluginRegistrant", "io.flutter")
            .forEach { forbidden -> assertFalse(service.contains(forbidden)) }
    }

    @Test
    fun `pigeon boundary contains domain operations and no raw security primitives`() {
        val schema = plugin.resolve("pigeons/repose_unlock_api.dart").toFile().readText()
        val generatedDart = plugin.resolve("lib/src/repose_unlock_api.g.dart").toFile().readText()
        val generatedKotlin = (
            plugin.resolve(
                "android/src/main/kotlin/ai/repose/mobile/unlock/generated/ReposeUnlockApi.g.kt",
            )
        ).toFile().readText()
        val methods = listOf(
            "getSnapshot",
            "requestCompanionAssociation",
            "beginPairing",
            "confirmPairing",
            "startCalibration",
            "submitCalibrationStep",
            "revokeDevice",
            "getDiagnostics",
        )

        assertTrue(schema.contains("@HostApi()"))
        methods.forEach { method ->
            assertEquals("schema method $method", 1, Regex("\\b$method\\s*\\(").findAll(schema).count())
            assertTrue("generated Dart method $method", generatedDart.contains("$method("))
            assertTrue("generated Kotlin method $method", generatedKotlin.contains("$method("))
        }
        listOf(
            "rawBle",
            "startGatt",
            "openChannel",
            "sendBytes",
            "signDigest",
            "configureAssociation",
            "associationId",
        )
            .forEach { forbidden ->
                assertFalse(Regex("\\b$forbidden\\s*\\(").containsMatchIn(schema))
            }
    }

    @Test
    fun `every host method shares one serial background task queue`() {
        val schema = plugin.resolve("pigeons/repose_unlock_api.dart").toFile().readText()
        val generatedKotlin = (
            plugin.resolve(
                "android/src/main/kotlin/ai/repose/mobile/unlock/generated/ReposeUnlockApi.g.kt",
            )
        ).toFile().readText()

        assertEquals(
            8,
            Regex("@TaskQueue\\(type: TaskQueueType\\.serialBackgroundThread\\)")
                .findAll(schema)
                .count(),
        )
        assertEquals(1, Regex("makeBackgroundTaskQueue\\(\\)").findAll(generatedKotlin).count())
        assertEquals(8, Regex(", codec, taskQueue\\)").findAll(generatedKotlin).count())
    }

    @Test
    fun `plugin registers and tears down a variant isolated host api`() {
        val pluginSource = plugin.resolve(
            "android/src/main/kotlin/ai/repose/mobile/unlock/ReposeUnlockNativePlugin.kt",
        ).toFile().readText()
        val debugFactory = plugin.resolve(
            "android/src/debug/kotlin/ai/repose/mobile/unlock/" +
                "VariantReposeUnlockHostApiFactory.kt",
        ).toFile().readText()
        val releaseFactory = plugin.resolve(
            "android/src/release/kotlin/ai/repose/mobile/unlock/" +
                "VariantReposeUnlockHostApiFactory.kt",
        ).toFile().readText()
        val profileFactory = plugin.resolve(
            "android/src/profile/kotlin/ai/repose/mobile/unlock/" +
                "VariantReposeUnlockHostApiFactory.kt",
        ).toFile().readText()

        assertTrue(pluginSource.contains("ReposeUnlockHostApi.setUp("))
        assertTrue(pluginSource.contains("VariantReposeUnlockHostApiFactory.create("))
        assertFalse(pluginSource.contains("DebugReposeUnlockHostApi"))
        assertTrue(debugFactory.contains("DebugReposeUnlockHostApi("))
        assertFalse(debugFactory.contains("FailClosedReposeUnlockHostApi("))
        assertTrue(releaseFactory.contains("FailClosedReposeUnlockHostApi("))
        assertFalse(releaseFactory.contains("DebugReposeUnlockHostApi"))
        assertTrue(profileFactory.contains("FailClosedReposeUnlockHostApi("))
        assertFalse(profileFactory.contains("DebugReposeUnlockHostApi"))
        val mainSources = Files.walk(plugin.resolve("android/src/main/kotlin")).use { paths ->
            paths.iterator().asSequence()
                .filter { Files.isRegularFile(it) && it.toString().endsWith(".kt") }
                .map { it.toFile().readText() }
                .joinToString("\n")
        }
        assertFalse(mainSources.contains("class DebugReposeUnlockHostApi"))
        assertFalse(mainSources.contains("class AndroidDebugGattPairingTransport"))
        assertFalse(mainSources.contains("class DebugPairingCoordinator"))
        assertTrue(
            Regex("onDetachedFromEngine[\\s\\S]*?ReposeUnlockHostApi\\.setUp\\([\\s\\S]*?null")
                .containsMatchIn(pluginSource),
        )
        assertTrue(pluginSource.contains("hostApi?.close()"))
        assertTrue(pluginSource.contains("ActivityAware"))
        assertTrue(pluginSource.contains("addRequestPermissionsResultListener"))
        assertTrue(pluginSource.contains("addActivityResultListener"))
        assertTrue(pluginSource.contains("VariantReposeUnlockHostApiFactory.configureAssociation("))

        assertTrue(debugFactory.contains("AndroidDebugCompanionAssociationProvider"))
        assertFalse(debugFactory.contains("ReposeUnlockRuntime.activeAssociationId"))
        assertFalse(debugFactory.contains("ReposeUnlockRuntime.availability"))
        assertTrue(releaseFactory.contains("ReposeUnlockRuntime.configureAssociation"))
        assertTrue(profileFactory.contains("ReposeUnlockRuntime.configureAssociation"))
        assertFalse(releaseFactory.contains("AndroidDebugCompanionAssociationProvider"))
        assertFalse(profileFactory.contains("AndroidDebugCompanionAssociationProvider"))
    }

    @Test
    fun `debug foreground association provider supports API 31 through 35 without presence runtime`() {
        val source = plugin.resolve(
            "android/src/debug/kotlin/ai/repose/mobile/unlock/pairing/" +
                "AndroidDebugCompanionAssociationProvider.kt",
        ).toFile().readText()

        assertTrue(source.contains("FEATURE_COMPANION_DEVICE_SETUP"))
        assertTrue(source.contains("Build.VERSION.SDK_INT >= 33"))
        assertTrue(source.contains("Build.VERSION.SDK_INT >= 34"))
        assertTrue(source.contains("manager.myAssociations"))
        assertTrue(source.contains("manager.associations"))
        assertTrue(source.contains("DebugAssociationSelector.select"))
        assertTrue(source.contains("legacyAssociationToken"))
        assertFalse(source.contains("ReposeUnlockRuntime"))

        val driver = plugin.resolve(
            "android/src/main/kotlin/ai/repose/mobile/unlock/companion/" +
                "AndroidCompanionAssociationDriver.kt",
        ).toFile().readText()
        assertTrue(driver.contains("CompanionDeviceManager.EXTRA_DEVICE"))
        assertTrue(driver.contains("legacyAssociationToken"))
    }

    @Test
    fun `debug GATT client resolves only its association and fixed Repose profile`() {
        val source = plugin.resolve(
            "android/src/debug/kotlin/ai/repose/mobile/unlock/pairing/" +
                "AndroidDebugGattPairingTransport.kt",
        ).toFile().readText()

        assertTrue(source.contains("manager.myAssociations.singleOrNull"))
        assertTrue(source.contains("@TargetApi(31)"))
        assertTrue(source.contains("Build.VERSION.SDK_INT >= 34"))
        assertTrue(source.contains("it.id == associationToken"))
        assertTrue(source.contains("association.associatedDevice?.bleDevice?.device"))
        assertTrue(source.contains("association.deviceMacAddress?.toString()"))
        assertTrue(source.contains("adapter::getRemoteDevice"))
        assertTrue(source.contains("BluetoothDevice.TRANSPORT_LE"))
        assertTrue(source.contains("requestMtu(TARGET_MTU)"))
        assertTrue(source.contains("PairingProtocolV1.bleServiceUuid"))
        assertTrue(source.contains("PairingProtocolV1.bleControlCharacteristicUuid"))
        assertTrue(source.contains("PairingProtocolV1.bleStatusCharacteristicUuid"))
        assertTrue(source.contains("BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE"))
        assertTrue(source.contains("readCharacteristic(statusValue)"))
        assertTrue(source.contains("CALLBACK_HANDLER.post(observer::onFailure)"))
        assertTrue(source.contains("CALLBACK_HANDLER.post(observer::onDisconnected)"))
        assertTrue(source.contains("HANDSHAKE_TIMEOUT_MILLIS = 15_000L"))
        assertTrue(source.contains("DebugGattHandshakeDeadline("))
        assertTrue(source.contains("handler.postDelayed"))
        assertTrue(source.contains("manager.associations"))
        assertTrue(source.contains("legacyAssociationToken"))
    }

    @Test
    fun `association discovery filters the fixed Repose service uuid`() {
        val sources = Files.walk(plugin.resolve("android/src/main/kotlin")).use { paths ->
            paths.iterator().asSequence()
                .filter { Files.isRegularFile(it) && it.toString().endsWith(".kt") }
                .map { it.toFile().readText() }
                .toList()
        }
        val joined = sources.joinToString("\n")

        assertTrue(joined.contains("A53E0001-7A6B-4D59-9F2E-5245504F5345"))
        assertTrue(joined.contains("BluetoothLeDeviceFilter.Builder()"))
        assertTrue(joined.contains("ScanFilter.Builder()"))
        assertTrue(joined.contains(".setServiceUuid("))
        assertTrue(joined.contains("AssociationRequest.Builder()"))
    }

    @Test
    fun `keystore adapter encodes the exact hardware policy`() {
        val source = (
            plugin.resolve(
                "android/src/main/kotlin/ai/repose/mobile/unlock/crypto/AndroidKeyStoreSigner.kt",
            )
        ).toFile().readText()

        assertTrue(source.contains("KeyProperties.KEY_ALGORITHM_EC"))
        assertTrue(source.contains("ECGenParameterSpec(SigningKeyPolicy.curveName)"))
        assertTrue(source.contains(".setDigests(KeyProperties.DIGEST_NONE)"))
        assertTrue(source.contains(".setUserAuthenticationRequired(false)"))
        assertTrue(source.contains(".setUnlockedDeviceRequired(false)"))
        assertTrue(source.contains("KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT"))
        assertTrue(source.contains("KeyProperties.SECURITY_LEVEL_STRONGBOX"))
    }

    private fun org.w3c.dom.NodeList.asElements(): List<Element> =
        (0 until length).map { item(it) as Element }
}
