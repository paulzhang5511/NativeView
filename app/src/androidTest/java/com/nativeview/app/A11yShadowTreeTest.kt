package com.nativeview.app

import android.graphics.Rect
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Inkflow 五屏流程的无障碍端到端测试。
 *
 * # 为什么用 UiAutomation 而非 TalkBack
 * `UiAutomation` 走的是与 TalkBack 相同的系统交互链路
 * （AccessibilityInteractionController → 应用 AccessibilityNodeProvider），
 * 因此能确定性地验证虚拟节点树与 `ACTION_CLICK` 回推，不依赖像素截图。
 *
 * # 覆盖点
 * 1. 作品库节点（新建 + 示例作品）可从系统侧枚举，bounds 非空；
 * 2. 点击链路能真正走完「作品库 → 新建 → 画板 → 导出 → 返回」；
 * 3. 画板新增的 dock/工具栏节点（笔刷、工具、图层、尺寸条、快捷笔刷）都暴露给无障碍。
 */
@RunWith(AndroidJUnit4::class)
class A11yShadowTreeTest {

    /** 被测 Activity 场景（保证前台窗口确为 NativeView）。 */
    private lateinit var scenario: ActivityScenario<MyNativeActivity>

    /** 取与 Activity 同源的自动化实例。 */
    private val automation get() = InstrumentationRegistry.getInstrumentation().uiAutomation

    /**
     * 显式启动 Activity：`rootInActiveWindow` 取系统当前前台窗口，
     * 若依赖外部启动，测试可能在别的应用前台时枚举节点。
     *
     * 首帧需要字体加载 + 后端初始化（模拟器软件光栅化较慢），给 20s 上限轮询。
     */
    @Before
    fun launchApp() {
        scenario = ActivityScenario.launch(MyNativeActivity::class.java)
        assertTrue("作品库未在 20s 内出现", awaitDesc("新建画布", 20000) != null)
    }

    /** 每个用例独立实例：销毁路径已有 LifecycleTest 回归覆盖。 */
    @After
    fun closeApp() {
        scenario.close()
    }

    /**
     * 等待指定 content-desc 的节点出现。
     *
     * 自绘页面把语义放在 contentDescription（节点无 text），
     * 两种字段都要匹配，否则 findAccessibilityNodeInfosByText 命中不了。
     */
    private fun awaitDesc(desc: String, timeoutMs: Long = 8000): AccessibilityNodeInfo? {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            val root = automation.rootInActiveWindow
            if (root != null) {
                val hits = root.findAccessibilityNodeInfosByText(desc)
                val hit = hits?.firstOrNull {
                    it.text?.toString() == desc || it.contentDescription?.toString() == desc
                }
                if (hit != null) return hit
            }
            Thread.sleep(100)
        }
        return null
    }

    /** 点击指定语义节点；节点在树重建后会失效，因此每次重新查找。 */
    private fun click(desc: String, timeoutMs: Long = 8000): Boolean {
        val node = awaitDesc(desc, timeoutMs) ?: return false
        return node.performAction(AccessibilityNodeInfo.ACTION_CLICK)
    }

    /** 作品库必须暴露 新建 + 3 张示例作品，且 bounds 可用。 */
    @Test
    fun libraryExposesSemanticNodes() {
        val plus = awaitDesc("新建画布")
        assertTrue("新建画布节点缺失", plus != null)
        val bounds = Rect()
        plus!!.getBoundsInScreen(bounds)
        assertFalse("bounds 为空会让 TalkBack 无法命中: $bounds", bounds.isEmpty)
        assertTrue("新建画布必须可点击", plus.isClickable)
        for (i in 1..3) {
            assertTrue("示例作品 $i 缺失", awaitDesc("示例作品 $i") != null)
        }
    }

    /** 画板必须把 dock 与工具栏的每个语义按钮暴露出去。 */
    @Test
    fun boardExposesDockAndToolNodes() {
        assertTrue("进入新建页失败", click("新建画布"))
        assertTrue("开始绘画按钮缺失", awaitDesc("创建画布", 12000) != null)
        assertTrue("进入画板失败", click("创建画布", 12000))

        // 顶栏 + 画布浮层 + dock 全部节点必须在 12s 内出现。
        val expected = listOf(
            "撤销", "重做", "导出 PNG", "笔刷面板", "切换图层", "画布尺寸",
            "画笔", "直线", "橡皮", "取色", "移动画布",
            "铅笔", "软笔", "喷枪", "水彩",
            "颜色 1", "颜色 7",
        )
        for (label in expected) {
            assertTrue("画板缺少节点: $label", awaitDesc(label, 12000) != null)
        }
    }

    /** 点击链路：作品库 → 新建 → 画板 → 导出 → 保存 → 返回作品库。 */
    @Test
    fun a11yClickWalksFiveScreens() {
        assertTrue("新建画布点击失败", click("新建画布"))

        // 新建页：选尺寸 2 / 纸色 3，并且每次点击都不离开新建页。
        assertTrue("开始绘画未出现", awaitDesc("创建画布", 12000) != null)
        assertTrue("画布尺寸 2 点击失败", click("画布尺寸 2"))
        assertTrue("纸色 3 点击失败", click("纸色 3", 8000))
        assertTrue("选尺寸后新建页消失", awaitDesc("创建画布", 8000) != null)

        // 画板：导出按钮在顶栏，点它应切到导出页。
        assertTrue("开始绘画点击失败", click("创建画布", 12000))
        assertTrue("画板未进入", awaitDesc("撤销", 12000) != null)
        assertTrue("导出按钮点击失败", click("导出 PNG", 12000))
        assertTrue("导出页未出现", awaitDesc("保存", 12000) != null)

        // 保存后会多出「返回作品库」，点它回到作品库。
        assertTrue("保存点击失败", click("保存", 12000))
        assertTrue("导出结果未出现", awaitDesc("返回作品库", 20000) != null)
        assertTrue("返回作品库点击失败", click("返回作品库", 12000))
        assertTrue("未回到作品库", awaitDesc("新建画布", 12000) != null)
    }
}
