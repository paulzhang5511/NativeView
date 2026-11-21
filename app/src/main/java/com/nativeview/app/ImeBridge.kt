package com.nativeview.app

import android.content.Context
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.CursorAnchorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import timber.log.Timber

/**
 * 隐形 EditText 桥（SPEC-ime.md §1）。
 *
 * 职责边界（本类铁律）：
 * - 仅承载 IME 交互，**不做任何文本渲染**（渲染全在 Rust 侧）；
 * - 1px 透明、默认 GONE，不参与用户可见布局；
 * - `TextWatcher` 逐事件回推 Rust（含 composing 区间），**绝不在回调线程
 *   调渲染**——Rust 侧只登记状态，重绘由 Choreographer 帧回调驱动。
 *
 * 生命周期：随 Activity 创建，随 Activity 销毁解绑。
 */
class ImeBridge(context: Context) {

    /** 隐形编辑框（1px 透明，默认不可见）。 */
    private val editText = object : EditText(context) {
        override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
            val ic = super.onCreateInputConnection(outAttrs) ?: return null
            // 下发 Rust 侧请求的 inputType / imeOptions（EditorInfo 由上层 setInputType 设定，
            // 此处仅确保 imeOptions 与单行语义正确）。
            outAttrs.imeOptions = outAttrs.imeOptions or EditorInfo.IME_FLAG_NO_FULLSCREEN
            return ic
        }
    }

    /** IME 管理器。 */
    private val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager

    /** 主线程 Handler（Rust 回调线程 → UI 线程转发）。 */
    private val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())

    /** 当前是否处于聚焦态（避免重复 show/hide）。 */
    private var focused = false

    /** 当前回推节流：仅文本/区间实际变化才回推。 */
    private var lastText: String = ""
    private var lastComposing: Pair<Int, Int>? = null

    init {
        // 1px 透明：不占视觉空间，但仍可接收 IME。
        editText.layoutParams = FrameLayout.LayoutParams(1, 1)
        editText.alpha = 0f
        editText.gravity = Gravity.TOP or Gravity.START
        editText.isSingleLine = true
        editText.visibility = View.GONE
        editText.setBackgroundColor(0)
        editText.setPadding(0, 0, 0, 0)
        // 焦点能力：NativeActivity 下 DecorView 默认吞焦点，必须显式声明。
        editText.isFocusable = true
        editText.isFocusableInTouchMode = true

        editText.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit

            override fun afterTextChanged(s: Editable?) {
                // 逐事件回推（拼音预编辑必须实时同步，不等确认）。
                pushText(s?.toString().orEmpty())
            }
        })
    }

    /** 挂载到窗口（Activity 根布局）。 */
    fun attachTo(parent: ViewGroup) {
        if (editText.parent == null) {
            parent.addView(editText)
        }
    }

    /** 解绑（Activity 销毁）。 */
    fun detach() {
        hideKeyboard()
        (editText.parent as? ViewGroup)?.removeView(editText)
    }

    /**
     * Rust 下发：聚焦并弹出键盘。
     *
     * # 线程
     * Rust 主循环线程调用，View 操作必须转发到 Android 主线程
     *（否则抛 "Only the original thread that created a view hierarchy"）。
     *
     * @param inputType Android `EditorInfo.inputType` 位值（Rust 侧契约）。
     * @param imeOptions Android `EditorInfo.imeOptions` 位值。
     */
    fun show(inputType: Int, imeOptions: Int, text: String) {
        mainHandler.post {
            editText.inputType = normalizeInputType(inputType)
            editText.imeOptions = imeOptions
            if (focused) {
                // 已聚焦：仅更新配置与文本（键盘面板类型切换/宿主文本变化）。
                syncText(text)
                return@post
            }
            focused = true
            editText.isFocusable = true
            editText.isFocusableInTouchMode = true
            editText.visibility = View.VISIBLE
            mainHandler.post {
                // 不 clearFocus：先灌入宿主文本再请求焦点，避免重聚焦清空输入。
                syncText(text)
                editText.requestFocus()
                val ok = imm.showSoftInput(editText, InputMethodManager.SHOW_IMPLICIT)
                Timber.i("ime bridge: showSoftInput=%s, chars=%d", ok, text.length)
            }
        }
    }

    /**
     * 把 Rust 侧文本灌入隐形 EditText（仅内容不同才写，避免 TextWatcher 回环）。
     *
     * 光标固定到末尾（V1 无选区模型）。
     */
    private fun syncText(text: String) {
        if (editText.text.toString() != text) {
            editText.setText(text)
            editText.setSelection(text.length)
        } else if (editText.selectionStart != text.length) {
            editText.setSelection(text.length)
        }
    }

    /**
     * Rust 下发插入点锚点（屏幕坐标 px）：转 `CursorAnchorInfo` 供候选窗/放大镜跟随。
     *
     * # 线程
     * Rust 主循环线程调用；IMM 调用必须转发主线程。
     */
    fun onCursorAnchor(x: Float, y: Float, height: Float) {
        mainHandler.post {
            if (!focused) return@post
            val info = CursorAnchorInfo.Builder()
                .setInsertionMarkerLocation(x, y, y + height, y + height, 0)
                .build()
            imm.updateCursorAnchorInfo(editText, info)
        }
    }

    /** Rust 下发：收起键盘并失焦（转发主线程）。 */
    fun hide() {
        mainHandler.post {
            if (!focused) return@post
            focused = false
            hideKeyboard()
            editText.visibility = View.GONE
            editText.clearFocus()
        }
    }

    /**
     * 窗口失焦 / Activity 暂停：系统可能强收键盘，需同步状态并通知 Rust。
     */
    fun onWindowFocusLost() {
        if (!focused) return
        focused = false
        editText.visibility = View.GONE
        nativeOnImeHidden()
    }

    /**
     * 键盘避让：把 IME 高度转发 Rust（布局层据此避让）。
     *
     * @param heightPx IME 遮挡高度（0 = 收起）。
     */
    fun forwardImeInset(heightPx: Int) {
        nativeOnImeInset(heightPx)
    }

    /** 关闭软键盘。 */
    private fun hideKeyboard() {
        imm.hideSoftInputFromWindow(editText.windowToken, 0)
    }

    /**
     * 回推文本与 composing 区间到 Rust。
     *
     * composing 区间来自 `BaseInputConnection.getComposingSpanStart/End`；
     * EditText 内部通过 SpellChecker 维持。这里用 `BaseInputConnection`
     * 静态方法读当前 editable 的 composing span。
     */
    private fun pushText(text: String) {
        val start = BaseInputConnection.getComposingSpanStart(editText.text)
        val end = BaseInputConnection.getComposingSpanEnd(editText.text)
        val composing = if (start >= 0 && end >= 0 && start != end) (start to end) else null
        if (text == lastText && composing == lastComposing) {
            return
        }
        lastText = text
        lastComposing = composing
        nativeOnImeText(text, start, end)
    }

    /**
     * Android 要求文本类 inputType 必须带 class 位；Rust 侧若只给变体位
     * （如仅 0x81），这里补齐 class 保证面板正确。
     */
    private fun normalizeInputType(inputType: Int): Int {
        val cls = inputType and InputType.TYPE_MASK_CLASS
        return if (cls == 0) {
            inputType or InputType.TYPE_CLASS_TEXT
        } else {
            inputType
        }
    }

    // ---- JNI：Rust 实现 ----
    private external fun nativeOnImeText(text: String, composingStart: Int, composingEnd: Int)
    private external fun nativeOnImeHidden()
    private external fun nativeOnImeInset(heightPx: Int)
}
