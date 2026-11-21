package com.nativeview.app

import android.app.NativeActivity
import android.content.Context
import android.os.Bundle
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.view.accessibility.AccessibilityManager

/**
 * 路线 B 入口（PRD §5.6.2）：NativeActivity 的 Kotlin 子类。
 *
 * 职责边界（本类铁律）：
 * - 仅作为渲染入口与桥接层宿主，禁止任何业务 UI 逻辑；
 * - 窗口绘制由 Rust 侧全权接管（NativeActivity#onCreate 中 takeSurface）；
 * - IME 桥（[ImeBridge]）在此挂载：仅传递平台输入参数，不做文本渲染。
 *
 * 系统栏 insets 注入：Android 15+ 强制 edge-to-edge，窗口 surface 恒全屏，
 * Rust 侧拿不到 insets 回调；此处经监听器读取后经 JNI 注入，Demo 布局据此
 * 避让状态栏/导航栏（仅平台参数传递，无业务 UI）。
 *
 * 时序保证：insets 分发早于首个 Choreographer 帧，而 Rust 首帧由
 * onNativeWindowCreated 驱动，两者均在 onResume 之后；即使注入晚于
 * 首帧，交互路径按需重读 insets，布局即时生效。
 */
class MyNativeActivity : NativeActivity() {

    /** 隐形 IME 桥（1px 透明 EditText；Rust 侧发起 focus 时弹出键盘）。 */
    private lateinit var imeBridge: ImeBridge

    /** 无障碍影子树桥（TalkBack 节点查询与点击回推）。 */
    private lateinit var a11yBridge: A11yBridge

    /** 无障碍服务开关监听：关闭时 Rust 侧零快照开销。 */
    private val a11yStateListener =
        AccessibilityManager.AccessibilityStateChangeListener { enabled ->
            nativeSetA11yEnabled(enabled)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 软键盘模式：adjustResize 让 IME 高度经 WindowInsets 上报，
        // 并确保窗口允许 IME 附着（NativeActivity 默认不声明）。
        window.setSoftInputMode(
            WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE or
                WindowManager.LayoutParams.SOFT_INPUT_STATE_UNCHANGED
        )

        // 密度注入：dp/sp 换算基准（用户字体缩放经 scaledDensity 体现）。
        val dm = resources.displayMetrics
        setDisplayDensity(dm.density, dm.scaledDensity)

        // IME 桥：挂到窗口 decorView（不可见的 1px 子视图）。
        imeBridge = ImeBridge(this)
        val root = window.decorView as ViewGroup
        imeBridge.attachTo(root)
        nativeAttachImeBridge(imeBridge)

        // 无障碍桥：零尺寸宿主 + NodeProvider（TalkBack 关闭时零开销）。
        a11yBridge = A11yBridge(this)
        a11yBridge.attachTo(root)
        nativeAttachA11yBridge(a11yBridge)
        // 上报当前无障碍开关（TalkBack 已开时立即建快照；关闭时零开销）。
        val am = getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        nativeSetA11yEnabled(am.isEnabled)
        am.addAccessibilityStateChangeListener(a11yStateListener)

        // insets 首次分发时注入 Rust（rootWindowInsets 此时尚未就绪，回调式最稳）。
        window.decorView.setOnApplyWindowInsetsListener { v, insets ->
            val status = insets.getInsets(WindowInsets.Type.statusBars()).top
            val nav = insets.getInsets(WindowInsets.Type.navigationBars()).bottom
            // IME 高度转发：Rust 布局据此避让（键盘弹出时输入框不被遮挡）。
            val ime = insets.getInsets(WindowInsets.Type.ime()).bottom
            imeBridge.forwardImeInset(ime)
            setSystemBarInsets(status, nav)
            v.onApplyWindowInsets(insets)
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) {
            // 窗口失焦：系统可能强收键盘，同步桥状态并通知 Rust。
            imeBridge.onWindowFocusLost()
        }
    }

    override fun onDestroy() {
        val am = getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        am.removeAccessibilityStateChangeListener(a11yStateListener)
        nativeDetachA11yBridge()
        a11yBridge.detach()
        nativeDetachImeBridge()
        imeBridge.detach()
        super.onDestroy()
    }

    /** JNI 桥：velloview 库的 Kotlin 侧声明（Rust 实现）。 */
    private external fun setSystemBarInsets(statusBar: Int, navBar: Int)
    private external fun setDisplayDensity(density: Float, scaledDensity: Float)
    private external fun nativeAttachImeBridge(bridge: ImeBridge)
    private external fun nativeDetachImeBridge()
    private external fun nativeAttachA11yBridge(bridge: A11yBridge)
    private external fun nativeDetachA11yBridge()

    /** JNI：无障碍服务开关上报（Rust 侧门禁快照构建）。 */
    private external fun nativeSetA11yEnabled(enabled: Boolean)

    companion object {
        init {
            // NativeActivity 已由系统经 meta-data 加载 libvelloview；
            // 此处显式声明保证 JNI 符号在任何回调前就绪。
            System.loadLibrary("velloview")
        }
    }
}
