plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "com.nativeview.app"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "com.nativeview.app"
        minSdk = 29
        targetSdk = 37
        versionCode = 1
        versionName = "1.0"
        // 无障碍影子树端到端测试（androidTest）需要 AndroidJUnitRunner。
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            optimization {
                enable = true
                packageScope = setOf("androidx.**", "kotlin.**", "kotlinx.**")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    // 字体走 assets 交原生 mmap（fd），禁止 APK 压缩破坏随机访问语义。
    androidResources {
        noCompress += "ttf"
    }
}

dependencies {
    // AGENTS §2.1：Android 侧统一用 Timber（禁止裸 android.util.Log）。
    implementation(libs.timber)
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
}
