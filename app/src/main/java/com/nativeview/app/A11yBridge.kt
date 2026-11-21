package com.nativeview.app

import android.content.Context
import android.graphics.Rect
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityManager
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeProvider

/**
 * 无障碍影子树桥（SPEC-accessibility.md §2）。
 *
 * 职责边界（本类铁律）：
 * - 只承载 TalkBack 的节点查询与动作回推，**不做任何绘制**；
 * - 不接收任何像素/图像数据（PRD §6.1/§6.2）；
 * - TalkBack 未开启时零开销：不建节点、不缓存、不响应。
 *
 * 数据来源：Rust 侧在帧回调内推送扁平化快照（[onSnapshot]），
 * 每行一个节点，字段以 `\u001f` 分隔。
 */
class A11yBridge(context: Context) {

    /** 无障碍管理器（判断 TalkBack 是否开启）。 */
    private val am = context.getSystemService(Context.ACCESSIBILITY_SERVICE)
        as AccessibilityManager

    /** 节点宿主视图：零尺寸、透明，仅用于承载 NodeProvider。 */
    private val host: View = object : View(context) {
        override fun getAccessibilityNodeProvider(): AccessibilityNodeProvider =
            this@A11yBridge.provider
    }

    /** UI 线程消息泵：快照解析与节点写入必须在主线程（provider 也在主线程被查询）。 */
    private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())

    /** 当前快照节点（按 id 索引，仅主线程读写）。 */
    private var nodes: List<Node> = emptyList()

    /** parentId → 子节点 id 列表（一次构建，避免每次 createNodeInfo 全表扫描 O(n²)）。 */
    private var childrenByParent: Map<Int, List<Int>> = emptyMap()

    /**
     * 当前无障碍焦点节点 id（-1 表示无）。
     *
     * TalkBack 会查询 `isAccessibilityFocused` 决定是否再次下发
     * ACTION_ACCESSIBILITY_FOCUS；不回写该状态会导致焦点反复请求、
     * 朗读抖动与无法用探索手势移动焦点。
     */
    private var a11yFocusId: Int = -1

    /** 是否已请求过 Rust 侧补推快照（防止 provider 查询递归触发请求）。 */
    private var snapshotRequested = false

    /** 节点提供者。 */
    private val provider: AccessibilityNodeProvider = object : AccessibilityNodeProvider() {

        /**
         * 根节点：必须返回「宿主视图自身」的 info（`HOST_VIEW_ID`），
         * TalkBack 从这里开始向下列举虚拟子节点。若把根也当成虚拟节点，
         * 系统只会对 id=0 做焦点操作而永不遍历子节点。
         */
        override fun createAccessibilityNodeInfo(virtualViewId: Int): AccessibilityNodeInfo? {
            if (virtualViewId == HOST_VIEW_ID) {
                // 系统侧已建连但 Rust 闸门仍关闭（无障碍总开关未开，
                // 例如 UiAutomation 测试）：请求补推一次快照。
                // 空树时反复请求（Rust 侧只置一次标记，代价极低）：
                // 连接建立/进程重建后 provider 与快照推送存在时序差。
                if (nodes.isEmpty()) {
                    snapshotRequested = true
                    nativeRequestSnapshot()
                }
                val root = AccessibilityNodeInfo.obtain(host)
                host.onInitializeAccessibilityNodeInfo(root)
                root.className = "android.view.ViewGroup"
                root.setPackageName(host.context.packageName)
                // 把快照中的顶层节点（parentId < 0）挂为子节点。
                for (n in nodes) {
                    if (n.parentId < 0) {
                        root.addChild(host, n.id)
                    }
                }
                return root
            }
            val n = nodes.getOrNull(virtualViewId) ?: return null
            val info = AccessibilityNodeInfo.obtain()
            info.setSource(host, n.id)
            info.setPackageName(host.context.packageName)
            info.className = n.className
            info.text = n.text.ifEmpty { null }
            info.contentDescription = n.contentDescription.ifEmpty { null }
            info.isEnabled = n.enabled
            info.isClickable = n.clickable
            info.isCheckable = n.checked
            info.isChecked = n.checked
            info.isVisibleToUser = n.visible
            info.isFocusable = true
            info.isAccessibilityFocused = (n.id == a11yFocusId)
            info.isScreenReaderFocusable = true
            info.setBoundsInScreen(Rect(n.left, n.top, n.right, n.bottom))
            if (n.parentId >= 0) {
                info.setParent(host, n.parentId)
            } else {
                info.setParent(host)
            }
            // 子节点：查预建索引（O(1)），避免逐节点全表扫描。
            childrenByParent[n.id]?.forEach { child ->
                info.addChild(host, child)
            }
            if (n.clickable) {
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK)
            }
            return info
        }

        override fun performAction(
            virtualViewId: Int,
            action: Int,
            arguments: Bundle?
        ): Boolean {
            // 根节点（宿主视图本身）只接受焦点类动作，不参与业务点击。
            if (virtualViewId == HOST_VIEW_ID) {
                return action == AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS ||
                    action == AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS
            }
            val n = nodes.getOrNull(virtualViewId) ?: return false
            return when (action) {
                AccessibilityNodeInfo.ACTION_CLICK -> {
                    if (!n.clickable) return false
                    // 只把节点 id 交回 Rust；业务语义由 Rust 侧标签决定。
                    nativeOnNodeClick(n.id)
                    true
                }
                AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS -> {
                    // 必须先清除旧焦点，再设置新焦点并回发事件；
                    // 否则 TalkBack 无法确认焦点已转移，会重复下发同一动作。
                    val prev = a11yFocusId
                    a11yFocusId = n.id
                    if (prev != -1 && prev != n.id) {
                        sendFocusEvent(prev, false)
                    }
                    sendFocusEvent(n.id, true)
                    true
                }
                AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS -> {
                    if (a11yFocusId == n.id) {
                        a11yFocusId = -1
                        sendFocusEvent(n.id, false)
                    }
                    true
                }
                else -> false
            }
        }

        override fun findAccessibilityNodeInfosByText(
            text: String?,
            virtualViewId: Int
        ): MutableList<AccessibilityNodeInfo> {
            if (text.isNullOrEmpty()) return mutableListOf()
            val out = mutableListOf<AccessibilityNodeInfo>()
            for (n in nodes) {
                val hay = n.text.ifEmpty { n.contentDescription }
                if (hay.contains(text, ignoreCase = true)) {
                    createAccessibilityNodeInfo(n.id)?.let { out.add(it) }
                }
            }
            return out
        }
    }

    /**
     * 上报虚拟节点的无障碍焦点事件。
     *
     * 虚拟节点无自带 View，必须用 `setSource(host, id)` 绑定宿主再交由父视图
     * 派发；直接 `host.sendAccessibilityEvent` 会丢失虚拟 id，TalkBack 无法
     * 定位焦点节点，进而反复重发焦点请求。
     */
    private fun sendFocusEvent(nodeId: Int, focused: Boolean) {
        val event = android.view.accessibility.AccessibilityEvent.obtain(
            if (focused) android.view.accessibility.AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED
            else android.view.accessibility.AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED
        )
        event.packageName = host.context.packageName
        event.className = nodes.getOrNull(nodeId)?.className ?: "android.view.View"
        event.setSource(host, nodeId)
        event.isEnabled = true
        val parentParent = host.parent
        if (parentParent != null) {
            parentParent.requestSendAccessibilityEvent(host, event)
        } else {
            event.recycle()
        }
    }

    /**
     * 挂载到窗口。
     *
     * 必须是全屏尺寸：0x0 视图会被判为 `isVisibleToUser=false`，
     * 导致 TalkBack 命中测试直接拒绝聚焦虚拟节点。
     * 同时保持 `isClickable=false` 且无背景，触摸事件会穿透到下层
     * SurfaceView（ViewGroup 对未消费的子树会继续派发给后续子视图）。
     */
    fun attachTo(parent: ViewGroup) {
        if (host.parent == null) {
            host.layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
            host.isClickable = false
            host.isFocusable = false
            host.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
            parent.addView(host)
        }
    }

    /** 解绑。 */
    fun detach() {
        nodes = emptyList()
        snapshotRequested = false
        (host.parent as? ViewGroup)?.removeView(host)
    }

    /** 请求 Rust 侧补推快照（系统无障碍建连但总开关未开时使用）。 */
    private external fun nativeRequestSnapshot()

    /** TalkBack 是否开启（关闭时 Rust 侧应停止推送）。 */
    fun isEnabled(): Boolean = am.isEnabled

    /**
     * 接收 Rust 快照（帧回调内推送）。
     *
     * 格式：每行一节点，字段以 `\u001f` 分隔，顺序为
     * id, parentId, left, top, right, bottom, text, contentDescription,
     * className, clickable, enabled, checked, visible, actionTag。
     */
    fun onSnapshot(payload: String) {
        // 从 Rust 主循环线程调用：解析/写节点/发事件全部切到主线程，
        // 保证 provider 查询看到完整快照（原实现跨线程写非 volatile 字段，
        // 存在未同步发布与半构造列表风险）。
        mainHandler.post { publish(payload) }
    }

    /** 主线程：解析并发布快照。 */
    private fun publish(payload: String) {
        if (!isEnabled()) {
            // TalkBack 已关闭：丢弃快照，避免无谓的内存与通知开销。
            nodes = emptyList()
            childrenByParent = emptyMap()
            return
        }
        val parsed = ArrayList<Node>()
        for (line in payload.lineSequence()) {
            if (line.isEmpty()) continue
            val f = line.split('\u001f')
            if (f.size < 14) continue
            parsed.add(
                Node(
                    id = f[0].toIntOrNull() ?: continue,
                    parentId = f[1].toIntOrNull() ?: -1,
                    left = f[2].toIntOrNull() ?: 0,
                    top = f[3].toIntOrNull() ?: 0,
                    right = f[4].toIntOrNull() ?: 0,
                    bottom = f[5].toIntOrNull() ?: 0,
                    text = f[6],
                    contentDescription = f[7],
                    className = f[8],
                    clickable = f[9] == "1",
                    enabled = f[10] == "1",
                    checked = f[11] == "1",
                    visible = f[12] == "1",
                    actionTag = f[13].toIntOrNull() ?: -1
                )
            )
        }
        nodes = parsed
        childrenByParent = parsed.groupBy { it.parentId }
            .mapValues { entry -> entry.value.map { it.id } }
        // 通知系统内容已变化：对宿主视图发内容变更事件，
        // TalkBack 会重新查询 provider（V1 只发此粒度事件）。
        host.sendAccessibilityEvent(
            android.view.accessibility.AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED
        )
    }

    /** 单个影子节点。 */
    private data class Node(
        val id: Int,
        val parentId: Int,
        val left: Int,
        val top: Int,
        val right: Int,
        val bottom: Int,
        val text: String,
        val contentDescription: String,
        val className: String,
        val clickable: Boolean,
        val enabled: Boolean,
        val checked: Boolean,
        val visible: Boolean,
        val actionTag: Int
    )

    // ---- JNI：Rust 实现 ----
    private external fun nativeOnNodeClick(nodeId: Int)
}
