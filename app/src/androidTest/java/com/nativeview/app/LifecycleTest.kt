package com.nativeview.app

import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * 生命周期回归：Activity 正常销毁不得崩溃。
 *
 * # 历史缺陷
 * 回调迟到时 `shared_of` 解引用已清空的 `instance`，以及 onDestroy 二次
 * 进入会对空指针 `Arc::from_raw`（UB），在 Android 13 上表现为
 * `Scudo ERROR: invalid chunk state`（~InputQueue 内 abort）。
 */
@RunWith(AndroidJUnit4::class)
class LifecycleTest {

    @Test
    fun destroyDoesNotCrash() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        val scenario = ActivityScenario.launch(MyNativeActivity::class.java)
        // 等首帧/字体就绪（销毁路径必须覆盖“已完整初始化”的状态）。
        val deadline = System.currentTimeMillis() + 20000
        while (System.currentTimeMillis() < deadline) {
            if (automation.rootInActiveWindow != null) break
            SystemClock.sleep(100)
        }
        SystemClock.sleep(1500)
        scenario.close()
        // 进程若崩溃，instrumentation 会直接中断，不会执行到这里。
        SystemClock.sleep(1500)
        assertTrue("process must survive Activity destroy", true)
    }
}
