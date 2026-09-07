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
            7,
            Regex("@TaskQueue\\(type: TaskQueueType\\.serialBackgroundThread\\)")
                .findAll(schema)
                .count(),
        )
        assertEquals(1, Regex("makeBackgroundTaskQueue\\(\\)").findAll(generatedKotlin).count())
        assertEquals(7, Regex(", codec, taskQueue\\)").findAll(generatedKotlin).count())
    }

    @Test
    fun `plugin registers and tears down the fail closed host api`() {
        val pluginSource = plugin.resolve(
            "android/src/main/kotlin/ai/repose/mobile/unlock/ReposeUnlockNativePlugin.kt",
        ).toFile().readText()

        assertTrue(pluginSource.contains("ReposeUnlockHostApi.setUp("))
        assertTrue(Regex("FailClosedReposeUnlockHostApi\\s*[({]").containsMatchIn(pluginSource))
        assertTrue(
            Regex("onDetachedFromEngine[\\s\\S]*?ReposeUnlockHostApi\\.setUp\\([\\s\\S]*?null")
                .containsMatchIn(pluginSource),
        )
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
