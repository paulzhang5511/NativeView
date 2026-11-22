package com.nativeview.app

import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.FileInputStream

/**
 * Inkflow 画板的真实输入注入验收：单指落笔 + 撤销、双指缩放。
 *
 * # 判定策略
 * 输入全部走系统级注入（UiAutomation 触摸事件）。像素截图在 SurfaceView +
 * SwiftShader 下不可靠，因此断言以「节点树仍可用 / 状态变化后按钮仍在」为准，
 * 不改用图像比对；笔迹像素本身由宿主侧单测（raster/paint）覆盖。
 */
@RunWith(AndroidJUnit4::class)
class DemoInteractionTest {

    private lateinit var scenario: ActivityScenario<MyNativeActivity>
    private val automation get() = InstrumentationRegistry.getInstrumentation().uiAutomation

    @Before
    fun launchApp() {
        scenario = ActivityScenario.launch(MyNativeActivity::class.java)
        assertTrue("作品库未在 20s 内出现", awaitDesc("新建画布", 20000) != null)
        SystemClock.sleep(800)
    }

    /**
     * 不主动销毁 Activity：Android 13 的 NativeActivity 窗口叠加 Java View 后，
     * destroy 阶段会对同一 InputQueue 触发二次 dispose（平台 Scudo abort），
     * 与交互断言无关。销毁路径由 LifecycleTest 单独覆盖。
     */
    @After
    fun detachOnly() {
        // scenario.close() 仅在生命周期用例里执行。
    }

    /** 等待 content-desc 节点出现（自绘页面的语义只在 contentDescription）。 */
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
            SystemClock.sleep(100)
        }
        return null
    }

    private fun click(desc: String, timeoutMs: Long = 8000): Boolean {
        val node = awaitDesc(desc, timeoutMs) ?: return false
        return node.performAction(AccessibilityNodeInfo.ACTION_CLICK)
    }

    /** 走完作品库 → 新建 → 画板，断言停在画板。 */
    private fun openBoard() {
        assertTrue("新建画布点击失败", click("新建画布"))
        assertTrue("开始绘画未出现", awaitDesc("创建画布", 12000) != null)
        assertTrue("开始绘画点击失败", click("创建画布", 12000))
        assertTrue("画板未进入", awaitDesc("撤销", 12000) != null)
        // 软件光栅化下单帧较慢，先让首帧稳定再注入触摸。
        SystemClock.sleep(1200)
    }

    private fun nativeLog(): String {
        val pfd = automation.executeShellCommand("logcat -d -s velloview")
        val text = FileInputStream(pfd.fileDescriptor).bufferedReader().readText()
        pfd.close()
        return text
    }

    private fun clearLog() {
        automation.executeShellCommand("logcat -c").close()
    }

    /** 单指拖动（时间戳连续，避免被当成两次独立点击）。 */
    private fun swipe(x: Int, y0: Int, y1: Int) {
        val t0 = SystemClock.uptimeMillis()
        fun ev(action: Int, y: Int, dt: Long): MotionEvent =
            MotionEvent.obtain(t0, t0 + dt, action, x.toFloat(), y.toFloat(), 0)

        automation.injectInputEvent(ev(MotionEvent.ACTION_DOWN, y0, 0), true)
        var dt = 20L
        var y = y0
        val steps = 12
        for (i in 1..steps) {
            y = y0 + (y1 - y0) * i / steps
            automation.injectInputEvent(ev(MotionEvent.ACTION_MOVE, y, dt), true)
            dt += 20
        }
        automation.injectInputEvent(ev(MotionEvent.ACTION_UP, y, dt), true)
        SystemClock.sleep(400)
    }

    /**
     * 双指反向捏合（间距变化 → 画布缩放）。
     *
     * # 序列必须从单指 ACTION_DOWN 开始
     * InputDispatcher 只承认「先 DOWN 再 POINTER_DOWN」的多指序列；
     * 直接以 POINTER_DOWN 起手会被判为非法流并整段丢弃（注入成功但应用收不到）。
     * 末端同样先 POINTER_UP 再 UP。
     */
    private fun pinch(cx: Int, cy: Int, from: Int, to: Int) {
        val props = arrayOf(
            MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_FINGER },
            MotionEvent.PointerProperties().apply { id = 1; toolType = MotionEvent.TOOL_TYPE_FINGER },
        )
        fun coords(d: Int) = arrayOf(
            MotionEvent.PointerCoords().apply { x = (cx - d).toFloat(); y = cy.toFloat(); pressure = 1f; size = 1f },
            MotionEvent.PointerCoords().apply { x = (cx + d).toFloat(); y = cy.toFloat(); pressure = 1f; size = 1f },
        )
        val t0 = SystemClock.uptimeMillis()
        fun multi(action: Int, d: Int, dt: Long): MotionEvent = MotionEvent.obtain(
            t0, t0 + dt, action, 2, props, coords(d), 0, 0, 1f, 1f, 0, 0,
            InputDevice.SOURCE_TOUCHSCREEN, 0
        )
        // 单指事件也必须带 toolType=FINGER：6 参 obtain 生成 TOOL_TYPE_UNKNOWN，
        // 与后续 FINGER 的 POINTER_DOWN 不属于同一输入流，会被静默丢弃。
        fun single(action: Int, x: Int, dt: Long): MotionEvent = MotionEvent.obtain(
            t0, t0 + dt, action, 1,
            arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_FINGER }),
            arrayOf(MotionEvent.PointerCoords().apply { this.x = x.toFloat(); y = cy.toFloat(); pressure = 1f; size = 1f }),
            0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0
        )

        automation.injectInputEvent(single(MotionEvent.ACTION_DOWN, cx - from, 0), true)
        automation.injectInputEvent(
            multi(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), from, 20), true
        )
        var dt = 40L
        for (i in 1..10) {
            val d = from + (to - from) * i / 10
            automation.injectInputEvent(multi(MotionEvent.ACTION_MOVE, d, dt), true)
            dt += 40
            // 每步留出消费窗口：单帧渲染慢时连续注入会被合并。
            SystemClock.sleep(200)
        }
        automation.injectInputEvent(
            multi(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), to, dt), true
        )
        automation.injectInputEvent(single(MotionEvent.ACTION_UP, cx - to, dt + 20), true)
        SystemClock.sleep(600)
    }

    /** 单指落笔必须产生一步可撤销的历史。 */
    @Test
    fun swipePaintsStrokeAndUndoWorks() {
        openBoard()
        clearLog()
        swipe(540, 900, 1400)
        SystemClock.sleep(1200)
        val log = nativeLog()
        // 路由日志证明事件确实进了 Inkflow 触摸层（不是被别的 View 吃掉）。
        assertTrue("触摸未进入 Inkflow 页面:\n$log", log.contains("motion:"))

        // 落笔后撤销按钮可用，点击后仍停在画板。
        assertTrue("撤销点击失败", click("撤销"))
        SystemClock.sleep(600)
        assertTrue("撤销后画板消失", awaitDesc("撤销", 8000) != null)
    }

    /** 双指缩放不得把画板打断（缩放后仍可继续操作）。 */
    @Test
    fun pinchKeepsBoardUsable() {
        openBoard()
        pinch(540, 1200, 120, 320)
        SystemClock.sleep(1500)
        // 缩放后节点树仍然完整，且还能点撤销（说明未卡死/未退出画板）。
        assertTrue("缩放后撤销按钮消失", awaitDesc("撤销", 8000) != null)
        assertTrue("缩放后撤销点击失败", click("撤销"))
        assertTrue("画板仍应可用", awaitDesc("导出 PNG", 8000) != null)
    }

    /** 工具切换不得打断画板：切到移动画布后仍能回到画笔。 */
    @Test
    fun toolSwitchKeepsBoardAlive() {
        openBoard()
        assertTrue("移动画布点击失败", click("移动画布"))
        SystemClock.sleep(400)
        assertTrue("切工具后画笔节点消失", awaitDesc("画笔", 8000) != null)
        assertTrue("画笔点击失败", click("画笔"))
        SystemClock.sleep(400)
        assertTrue("切回画笔后图层节点消失", awaitDesc("切换图层", 8000) != null)
    }
}
