package com.nativeview.app

import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import android.view.accessibility.AccessibilityNodeInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 无障碍影子树端到端测试（SPEC-accessibility.md 成功标准 1/2）。
 *
 * # 为什么用 UiAutomation 而非 TalkBack
 * TalkBack 在一部分厂商设备上会被系统周期性回收，人工双击无法稳定复现；
 * `UiAutomation` 走的是**同一条系统无障碍交互链路**
 * （AccessibilityInteractionController → 应用的 AccessibilityNodeProvider），
 * 因此能确定性地验证虚拟节点树与 `ACTION_CLICK` 回推。
 *
 * # 覆盖点
 * 1. 虚拟节点树可从系统侧枚举（层级/文本/角色/可点击性/bounds 正确）；
 * 2. 派发 `ACTION_CLICK` 后 Rust 侧状态真实变更（页签切换）。
 */
@RunWith(AndroidJUnit4::class)
class A11yShadowTreeTest {

    /** 被测 Activity 场景（保证前台窗口确为 NativeView）。 */
    private lateinit var scenario: ActivityScenario<MyNativeActivity>

    /** 取与 Activity 同源的自动化实例。 */
    private val automation get() = InstrumentationRegistry.getInstrumentation().uiAutomation

    /**
     * 显式启动 Activity：`rootInActiveWindow` 取的是系统当前前台窗口，
     * 若仅依赖安装后由外部启动，测试可能在别的应用前台时枚举节点。
     *
     * 首帧需要等待字体加载 + 后端初始化（模拟器 SwiftShader 较慢），
     * 这里给 20s 上限并轮询节点出现，而非固定 sleep。
     */
    @Before
    fun launchApp() {
        scenario = ActivityScenario.launch(MyNativeActivity::class.java)
        awaitNodeByText("List", timeoutMs = 20000)
    }

    /** 每个用例独立实例：销毁路径已有 LifecycleTest 回归覆盖。 */
    @After
    fun closeApp() {
        scenario.close()
    }

    /** 等待指定文本的虚拟节点出现（快照经帧回调异步推送）。 */
    private fun awaitNodeByText(text: String, timeoutMs: Long = 5000): AccessibilityNodeInfo? {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            val root = automation.rootInActiveWindow
            if (root != null) {
                val hits = root.findAccessibilityNodeInfosByText(text)
                val hit = hits?.firstOrNull { it.text?.toString() == text }
                if (hit != null) return hit
            }
            Thread.sleep(100)
        }
        return null
    }

    /** 活动窗口根节点必须能枚举出三个页签语义节点。 */
    @Test
    fun shadowTreeExposesTabs() {
        val list = awaitNodeByText("List")
        val draw = awaitNodeByText("Draw")
        val text = awaitNodeByText("Text")
        assertNotNull("List tab node must be visible to the a11y layer", list)
        assertNotNull("Draw tab node must be visible to the a11y layer", draw)
        assertNotNull("Text tab node must be visible to the a11y layer", text)
        assertEquals("android.widget.Button", draw!!.className)
        assertTrue("tab must be clickable", draw.isClickable)
        assertTrue("tab must be enabled", draw.isEnabled)
        // bounds 必须非空（空 bounds 会让 TalkBack 无法命中）。
        val r = android.graphics.Rect()
        draw.getBoundsInScreen(r)
        assertTrue("bounds must be non-empty: $r", !r.isEmpty)
    }

    /**
     * `ACTION_CLICK` 必须真正驱动 Rust 状态机（与手势路径同一通路）。
     *
     * # 断言方式
     * 点击 Draw 页签后，Draw 节点应变为 `isChecked`（页签选中态由 Rust 侧
     * `a11y_checked` 提供），这是不依赖截图/像素的确定性判据。
     */
    /** 点击页签后 Rust 侧 `a11y_checked` 翻转（不依赖截图的确定性判据）。 */
    @Test
    fun actionClickSwitchesPage() {
        val draw = awaitNodeByText("Draw")
        assertNotNull("Draw tab must exist before clicking", draw)
        assertTrue("click action must be performed", draw!!.performAction(AccessibilityNodeInfo.ACTION_CLICK))

        // 等 Rust 处理点击并重推快照。
        val deadline = System.currentTimeMillis() + 5000
        var checked = false
        while (System.currentTimeMillis() < deadline) {
            val node = awaitNodeByText("Draw", timeoutMs = 500)
            if (node != null && node.isChecked) {
                checked = true
                break
            }
            Thread.sleep(100)
        }
        assertTrue("Draw tab must become checked after ACTION_CLICK", checked)
    }
}
