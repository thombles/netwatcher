plugins {
    alias(libs.plugins.android.library)
    id("com.vanniktech.maven.publish") version "0.34.0"
}

android {
    namespace = "net.octet_stream.netwatcher.netwatcher_android"
    compileSdk = 36

    defaultConfig {
        minSdk = 21

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
        externalNativeBuild {
            cmake {
                cppFlags("")
                arguments += listOf("-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    externalNativeBuild {
        cmake {
            path("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
}

dependencies {
    implementation(libs.androidx.appcompat)
    implementation(libs.material)
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
}

// Maven Central publishing configuration
mavenPublishing {
    publishToMavenCentral()
    signAllPublications()
    coordinates("net.octet-stream.netwatcher", "netwatcher-android", "0.2.0")
    pom {
        name = "Netwatcher"
        description = "Android support library for netwatcher Rust crate"
        inceptionYear = "2025"
        url = "https://github.com/thombles/netwatcher/"
        licenses {
            license {
                name = "MIT License"
                url = "https://opensource.org/licenses/MIT"
                distribution = "https://opensource.org/licenses/MIT"
            }
        }
        developers {
            developer {
                id = "thombles"
                name = "Thomas Karpiniec"
                url = "https://github.com/thombles/"
            }
        }
        scm {
            url = "https://github.com/thombles/netwatcher/"
            connection = "scm:git:https://github.com/thombles/netwatcher.git"
            developerConnection = "scm:git:https://github.com/thombles/netwatcher.git"
        }
    }
}